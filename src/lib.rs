//! A realtime-safe single-producer single-consumer (SPSC) ring buffer.
//!
//! A [`RingBuffer`] consists of two parts:
//! a [`Producer`] for writing into the ring buffer and
//! a [`Consumer`] for reading from the ring buffer.
//!
//! A fixed-capacity buffer is allocated on construction.
//! After that, no more memory is allocated (unless the type `T` does that internally).
//! Reading from and writing into the ring buffer is *lock-free* and *wait-free*.
//! All reading and writing functions return immediately.
//! Attempts to write to a full buffer return an error;
//! values inside the buffer are *not* overwritten.
//! Attempts to read from an empty buffer return an error as well.
//! Only a single thread can write into the ring buffer and a single thread
//! (typically a different one) can read from the ring buffer.
//! If the queue is empty, there is no way for the reading thread to wait
//! for new data, other than trying repeatedly until reading succeeds.
//! Similarly, if the queue is full, there is no way for the writing thread
//! to wait for newly available space to write to, other than trying repeatedly.
//!
//! # Examples
//!
//! Moving single elements into and out of a queue with
//! [`Producer::push()`] and [`Consumer::pop()`], respectively:
//!
//! ```
//! use rtrb::{RingBuffer, PushError, PopError};
//!
//! let (mut producer, mut consumer) = RingBuffer::new(2);
//!
//! assert_eq!(producer.push(10), Ok(()));
//! assert_eq!(producer.push(20), Ok(()));
//! assert_eq!(producer.push(30), Err(PushError::Full(30)));
//!
//! std::thread::spawn(move || {
//!     assert_eq!(consumer.pop(), Ok(10));
//!     assert_eq!(consumer.pop(), Ok(20));
//!     assert_eq!(consumer.pop(), Err(PopError::Empty));
//! }).join().unwrap();
//! ```
//!
//! See the documentation of the [`chunks#examples`] module
//! for examples that write multiple items at once with
//! [`Producer::write_chunk_uninit()`] and [`Producer::write_chunk()`]
//! and read multiple items with [`Consumer::read_chunk()`].

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(rust_2018_idioms)]
#![deny(missing_docs, missing_debug_implementations)]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, MaybeUninit};
use core::sync::atomic::AtomicUsize;

use crossbeam_utils::CachePadded;

/*
pub mod chunks;

// This is used in the documentation.
#[allow(unused_imports)]
use chunks::WriteChunkUninit;
*/

use rtrb_base::{Addressing, Indices, Storage};

// TODO: use rtrb_base::errors::* or something?
pub use rtrb_base::{PushError, PopError};

/// A bounded single-producer single-consumer (SPSC) queue.
///
/// Elements can be written with a [`Producer`] and read with a [`Consumer`],
/// both of which can be obtained with [`RingBuffer::new()`].
///
/// *See also the [crate-level documentation](crate).*
pub type RingBuffer<T> = DynamicStorage<T, TightAddressing, CachePaddedIndices>;

/// TODO: move docs
pub type Consumer<T> = rtrb_base::Consumer<RingBuffer<T>>;

/// Dynamic storage on the heap.
#[derive(Debug)]
pub struct DynamicStorage<T, A: Addressing, I: Indices> {
    addr: A,
    indices: I,

    /// The buffer holding slots.
    data_ptr: *mut T,

    /// Indicates that dropping a `DynamicStorage<T, _>` may drop elements of type `T`.
    _marker: PhantomData<T>,
}

impl<T, A: Addressing, I: Indices> DynamicStorage<T, A, I> {
    /// Creates a ring buffer with the given `capacity` and returns [`Producer`] and [`Consumer`].
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (producer, consumer) = RingBuffer::<f32>::new(100);
    /// ```
    ///
    /// Specifying an explicit type with the [turbofish](https://turbo.fish/)
    /// is is only necessary if it cannot be deduced by the compiler.
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (mut producer, consumer) = RingBuffer::new(100);
    /// assert_eq!(producer.push(0.0f32), Ok(()));
    /// ```
    #[allow(clippy::new_ret_no_self)]
    #[must_use]
    pub fn new(capacity: usize) -> (rtrb_base::Producer<Self>, rtrb_base::Consumer<Self>) {
        let addr = A::new(capacity);
        let capacity = addr.capacity();
        let buffer = Arc::new(Self {
            addr,
            indices: I::new(),
            data_ptr: ManuallyDrop::new(Vec::with_capacity(capacity)).as_mut_ptr(),
            _marker: PhantomData,
        });
        // SAFETY: Only a single instance of Producer is allowed.
        let p = unsafe { rtrb_base::Producer::new(buffer.clone()) };
        let c = unsafe { rtrb_base::Consumer::new(buffer) };
        (p, c)
    }

    /// Returns the capacity of the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (producer, consumer) = RingBuffer::<f32>::new(100);
    /// assert_eq!(producer.buffer().capacity(), 100);
    /// assert_eq!(consumer.buffer().capacity(), 100);
    /// // Both producer and consumer of course refer to the same ring buffer:
    /// assert_eq!(producer.buffer(), consumer.buffer());
    /// ```
    pub fn capacity(&self) -> usize {
        self.addr.capacity()
    }
}

impl<T, A: Addressing, I: Indices> PartialEq for DynamicStorage<T, A, I> {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self, other)
    }
}

impl<T, A: Addressing, I: Indices> Eq for DynamicStorage<T, A, I> {}

// TODO: consider this an "unsafe" impl?
// all methods must be implemented correctly, or the whole thing is unsound
impl<T, A: Addressing, I: Indices> Storage for DynamicStorage<T, A, I> {
    type Item = T;
    type Addr = A;
    type Indices = I;

    type Reference = Arc<Self>;

    fn data_ptr(&self) -> *mut Self::Item {
        self.data_ptr
    }

    fn addr(&self) -> &Self::Addr {
        &self.addr
    }

    fn indices(&self) -> &Self::Indices {
        &self.indices
    }

    fn is_abandoned(this: &Self::Reference) -> bool {
        Arc::strong_count(this) < 2
    }
}

/// Padded indices to avoid false sharing.
#[derive(Debug)]
pub struct CachePaddedIndices {
    /// The head of the queue.
    ///
    /// This integer is in range `0 .. 2 * capacity`.
    head: CachePadded<AtomicUsize>,

    /// The tail of the queue.
    ///
    /// This integer is in range `0 .. 2 * capacity`.
    tail: CachePadded<AtomicUsize>,
}

impl Indices for CachePaddedIndices {
    fn new() -> Self {
        CachePaddedIndices {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
        }
    }

    #[inline]
    fn head(&self) -> &AtomicUsize {
        &self.head
    }

    #[inline]
    fn tail(&self) -> &AtomicUsize {
        &self.tail
    }
}

/// Exact length.
#[derive(Debug)]
pub struct TightAddressing {
    /// The queue capacity.
    capacity: usize,
}

impl Addressing for TightAddressing {
    fn new(capacity: usize) -> Self {
        Self { capacity }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    /// Wraps a position from the range `0 .. 2 * capacity` to `0 .. capacity`.
    #[inline]
    fn collapse_position(&self, pos: usize) -> usize {
        debug_assert!(pos == 0 || pos < 2 * self.capacity);
        if pos < self.capacity {
            pos
        } else {
            pos - self.capacity
        }
    }

    #[inline]
    fn increment(&self, pos: usize, n: usize) -> usize {
        debug_assert!(pos == 0 || pos < 2 * self.capacity);
        debug_assert!(n <= self.capacity);
        let threshold = 2 * self.capacity - n;
        if pos < threshold {
            pos + n
        } else {
            pos - threshold
        }
    }

    #[inline]
    fn increment1(&self, pos: usize) -> usize {
        debug_assert_ne!(self.capacity, 0);
        debug_assert!(pos < 2 * self.capacity);
        if pos < 2 * self.capacity - 1 {
            pos + 1
        } else {
            0
        }
    }

    #[inline]
    fn distance(&self, a: usize, b: usize) -> usize {
        debug_assert!(a == 0 || a < 2 * self.capacity);
        debug_assert!(b == 0 || b < 2 * self.capacity);
        if a <= b {
            b - a
        } else {
            2 * self.capacity - a + b
        }
    }
}

/// Force power of two.
#[derive(Debug)]
pub struct PowerOfTwoAddressing {
    /// The queue capacity (a power of 2).
    capacity: usize,
}

impl Addressing for PowerOfTwoAddressing {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.next_power_of_two(),
        }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    fn collapse_position(&self, pos: usize) -> usize {
        // TODO: is capacity 0 supported?
        pos & (self.capacity - 1)
    }

    #[inline]
    fn increment(&self, pos: usize, n: usize) -> usize {
        pos.wrapping_add(n)
    }

    // TODO: this is probably not more efficient?
    #[inline]
    fn increment1(&self, pos: usize) -> usize {
        pos.wrapping_add(1)
    }

    #[inline]
    fn distance(&self, a: usize, b: usize) -> usize {
        b.wrapping_sub(a)
    }
}

impl<T, A: Addressing, I: Indices> Drop for DynamicStorage<T, A, I> {
    /// Drops all non-empty slots.
    fn drop(&mut self) {
        self.drop_all_elements();

        // Finally, deallocate the buffer, but don't run any destructors.
        unsafe {
            Vec::from_raw_parts(self.data_ptr, 0, self.addr().capacity());
        }
    }
}

/// TODO: move docs
pub type Producer<T> = rtrb_base::Producer<RingBuffer<T>>;

/// Extension trait used to provide a [`copy_to_uninit()`](CopyToUninit::copy_to_uninit)
/// method on built-in slices.
///
/// This can be used to safely copy data to the slices returned from
/// [`WriteChunkUninit::as_mut_slices()`].
///
/// To use this, the trait has to be brought into scope, e.g. with:
///
/// ```
/// use rtrb::CopyToUninit;
/// ```
pub trait CopyToUninit<T: Copy> {
    /// Copies contents to a possibly uninitialized slice.
    fn copy_to_uninit<'a>(&self, dst: &'a mut [MaybeUninit<T>]) -> &'a mut [T];
}

impl<T: Copy> CopyToUninit<T> for [T] {
    /// Copies contents to a possibly uninitialized slice.
    ///
    /// # Panics
    ///
    /// This function will panic if the two slices have different lengths.
    fn copy_to_uninit<'a>(&self, dst: &'a mut [MaybeUninit<T>]) -> &'a mut [T] {
        assert_eq!(
            self.len(),
            dst.len(),
            "source slice length does not match destination slice length"
        );
        let dst_ptr = dst.as_mut_ptr().cast();
        unsafe {
            self.as_ptr().copy_to_nonoverlapping(dst_ptr, self.len());
            core::slice::from_raw_parts_mut(dst_ptr, self.len())
        }
    }
}
