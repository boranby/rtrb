#![no_std]
#![warn(rust_2018_idioms)]

use core::cell::Cell;
use core::fmt;
use core::ops::Deref;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Indices.
///
/// # Safety
///
/// The indices must not be changed by anyone else.
pub unsafe trait Indices {
    fn new() -> Self;

    fn head(&self) -> &AtomicUsize;
    fn tail(&self) -> &AtomicUsize;
}

/// Addressing.
///
/// # Safety
///
/// ...
pub unsafe trait Addressing {
    //type SizeType;
    // TODO: AtomicSizeType?

    fn new(capacity: usize) -> Self;

    fn capacity(&self) -> usize;

    fn collapse_position(&self, pos: usize) -> usize;

    /// Increments a position by going `n` slots forward.
    fn increment(&self, pos: usize, n: usize) -> usize;

    /// Increments a position by going one slot forward.
    ///
    /// This might be more efficient than self.increment(..., 1).
    #[inline]
    fn increment1(&self, pos: usize) -> usize {
        self.increment(pos, 1)
    }

    /// Returns the distance between two positions.
    fn distance(&self, a: usize, b: usize) -> usize;
}

/// Storage.
///
/// # Safety
///
/// Storage must be contiguous.
///
/// ...
pub unsafe trait Storage {
    type Item;
    type Addr: Addressing;
    type Indices: Indices;

    //type Reference: Deref<Target = Self>;

    fn data_ptr(&self) -> *mut Self::Item;

    fn addr(&self) -> &Self::Addr;

    fn indices(&self) -> &Self::Indices;

    #[inline(never)]
    fn drop_all_elements(&mut self) {
        let mut head = self.indices().head().load(Ordering::Relaxed);
        let tail = self.indices().tail().load(Ordering::Relaxed);

        // Loop over all slots that hold a value and drop them.
        while head != tail {
            unsafe {
                self.slot_ptr(head).drop_in_place();
            }
            head = self.addr().increment1(head);
        }
        // This is not needed if drop_all_elements() is only called once,
        // but to be safe, we call it anyway:
        self.indices().head().store(head, Ordering::Relaxed);
    }

    /// Returns a pointer to the slot at position `pos`.
    ///
    /// If `pos == 0 && capacity == 0`, the returned pointer must not be dereferenced!
    unsafe fn slot_ptr(&self, pos: usize) -> *mut Self::Item {
        self.data_ptr().add(self.addr().collapse_position(pos))
    }

    //fn is_abandoned(this: &Self::Reference) -> bool;
}

/// The producer side of a [`RingBuffer`].
///
/// Can be moved between threads,
/// but references from different threads are not allowed
/// (i.e. it is [`Send`] but not [`Sync`]).
///
/// Can only be created with [`RingBuffer::new()`]
/// (together with its counterpart, the [`Consumer`]).
///
/// Individual elements can be moved into the ring buffer with [`Producer::push()`],
/// multiple elements at once can be written with [`Producer::write_chunk()`]
/// and [`Producer::write_chunk_uninit()`].
///
/// The number of free slots currently available for writing can be obtained with
/// [`Producer::slots()`].
///
/// When the `Producer` is dropped, [`Consumer::is_abandoned()`] will return `true`.
/// This can be used as a crude way to communicate to the receiving thread
/// that no more data will be produced.
/// When the `Producer` is dropped after the [`Consumer`] has already been dropped,
/// [`RingBuffer::drop()`] will be called, freeing the allocated memory.
#[derive(Debug, PartialEq, Eq)]
pub struct Producer<R> {
    /// A reference to the ring buffer.
    buffer: R,

    /// A copy of `buffer.head` for quick access.
    ///
    /// This value can be stale and sometimes needs to be resynchronized with `buffer.head`.
    cached_head: Cell<usize>,
}

unsafe impl<S: Storage, R: Deref<Target = S>> Send for Producer<R> where S::Item: Send {}

impl<S: Storage, R: Deref<Target = S>> Producer<R> {
    #[doc(hidden)]
    pub unsafe fn new(buffer: R) -> Self {
        Self {
            buffer,
            cached_head: Cell::new(0),
        }
    }

    /// Attempts to push an element into the queue.
    ///
    /// The element is *moved* into the ring buffer and its slot
    /// is made available to be read by the [`Consumer`].
    ///
    /// # Errors
    ///
    /// If the queue is full, the element is returned back as an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::{RingBuffer, PushError};
    ///
    /// let (mut p, c) = RingBuffer::new(1);
    ///
    /// assert_eq!(p.push(10), Ok(()));
    /// assert_eq!(p.push(20), Err(PushError::Full(20)));
    /// ```
    pub fn push(&mut self, value: S::Item) -> Result<(), PushError<S::Item>> {
        if let Some(tail) = self.next_tail() {
            unsafe {
                self.buffer.slot_ptr(tail).write(value);
            }
            let tail = self.buffer.addr().increment1(tail);
            self.buffer.indices().tail().store(tail, Ordering::Release);
            Ok(())
        } else {
            Err(PushError::Full(value))
        }
    }

    /// Returns the number of slots available for writing.
    ///
    /// Since items can be concurrently consumed on another thread, the actual number
    /// of available slots may increase at any time (up to the [`RingBuffer::capacity()`]).
    ///
    /// To check for a single available slot,
    /// using [`Producer::is_full()`] is often quicker
    /// (because it might not have to check an atomic variable).
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (p, c) = RingBuffer::<f32>::new(1024);
    ///
    /// assert_eq!(p.slots(), 1024);
    /// ```
    pub fn slots(&self) -> usize {
        let head = self.buffer.indices().head().load(Ordering::Acquire);
        self.cached_head.set(head);
        // "tail" is only ever written by the producer thread, "Relaxed" is enough
        let tail = self.buffer.indices().tail().load(Ordering::Relaxed);
        self.buffer.addr().capacity() - self.buffer.addr().distance(head, tail)
    }

    /// Returns `true` if there are currently no slots available for writing.
    ///
    /// A full ring buffer might cease to be full at any time
    /// if the corresponding [`Consumer`] is consuming items in another thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (p, c) = RingBuffer::<f32>::new(1);
    ///
    /// assert!(!p.is_full());
    /// ```
    ///
    /// Since items can be concurrently consumed on another thread, the ring buffer
    /// might not be full for long:
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (p, c) = RingBuffer::<f32>::new(1);
    /// if p.is_full() {
    ///     // The buffer might be full, but it might as well not be
    ///     // if an item was just consumed on another thread.
    /// }
    /// ```
    ///
    /// However, if it's not full, another thread cannot change that:
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (p, c) = RingBuffer::<f32>::new(1);
    /// if !p.is_full() {
    ///     // At least one slot is guaranteed to be available for writing.
    /// }
    /// ```
    pub fn is_full(&self) -> bool {
        self.next_tail().is_none()
    }

    /// Returns the total capacity of the queue.
    ///
    /// At any time, the capacity is subdivided into
    /// [`Producer::slots()`] available for writing and
    /// [`Consumer::slots()`] available for reading.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (producer, consumer) = RingBuffer::<f32>::new(100);
    /// assert_eq!(producer.capacity(), 100);
    /// assert_eq!(consumer.capacity(), 100);
    /// ```
    pub fn capacity(&self) -> usize {
        self.buffer.addr().capacity()
    }

    /// Get the tail position for writing the next slot, if available.
    ///
    /// This is a strict subset of the functionality implemented in `write_chunk_uninit()`.
    /// For performance, this special case is immplemented separately.
    fn next_tail(&self) -> Option<usize> {
        let indices = self.buffer.indices();
        let addr = self.buffer.addr();
        // "tail" is only ever written by the producer thread, "Relaxed" is enough
        let tail = indices.tail().load(Ordering::Relaxed);

        // Check if the queue is *possibly* full.
        if addr.distance(self.cached_head.get(), tail) == addr.capacity() {
            // Refresh the head ...
            let head = indices.head().load(Ordering::Acquire);
            // ... and check if it's *really* full.
            if addr.distance(head, tail) == addr.capacity() {
                return None;
            }
            self.cached_head.set(head);
        }
        Some(tail)
    }
}

/// The consumer side of a [`RingBuffer`].
///
/// Can be moved between threads,
/// but references from different threads are not allowed
/// (i.e. it is [`Send`] but not [`Sync`]).
///
/// Can only be created with [`RingBuffer::new()`]
/// (together with its counterpart, the [`Producer`]).
///
/// Individual elements can be moved out of the ring buffer with [`Consumer::pop()`],
/// multiple elements at once can be read with [`Consumer::read_chunk()`].
///
/// The number of slots currently available for reading can be obtained with
/// [`Consumer::slots()`].
///
/// When the `Consumer` is dropped, [`Producer::is_abandoned()`] will return `true`.
/// This can be used as a crude way to communicate to the sending thread
/// that no more data will be consumed.
/// When the `Consumer` is dropped after the [`Producer`] has already been dropped,
/// [`RingBuffer::drop()`] will be called, freeing the allocated memory.
#[derive(Debug, PartialEq, Eq)]
pub struct Consumer<R> {
    /// A reference to the ring buffer.
    buffer: R,

    /// A copy of `buffer.tail` for quick access.
    ///
    /// This value can be stale and sometimes needs to be resynchronized with `buffer.tail`.
    cached_tail: Cell<usize>,
}

unsafe impl<S: Storage, R: Deref<Target = S>> Send for Consumer<R> where S::Item: Send {}

impl<S: Storage, R: Deref<Target = S>> Consumer<R> {
    #[doc(hidden)]
    pub unsafe fn new(buffer: R) -> Self {
        Self {
            buffer,
            cached_tail: Cell::new(0),
        }
    }

    /// Attempts to pop an element from the queue.
    ///
    /// The element is *moved* out of the ring buffer and its slot
    /// is made available to be filled by the [`Producer`] again.
    ///
    /// # Errors
    ///
    /// If the queue is empty, an error is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::{PopError, RingBuffer};
    ///
    /// let (mut p, mut c) = RingBuffer::new(1);
    ///
    /// assert_eq!(p.push(10), Ok(()));
    /// assert_eq!(c.pop(), Ok(10));
    /// assert_eq!(c.pop(), Err(PopError::Empty));
    /// ```
    ///
    /// To obtain an [`Option<T>`](Option), use [`.ok()`](Result::ok) on the result.
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (mut p, mut c) = RingBuffer::new(1);
    /// assert_eq!(p.push(20), Ok(()));
    /// assert_eq!(c.pop().ok(), Some(20));
    /// ```
    pub fn pop(&mut self) -> Result<S::Item, PopError> {
        if let Some(head) = self.next_head() {
            let value = unsafe { self.buffer.slot_ptr(head).read() };
            let head = self.buffer.addr().increment1(head);
            self.buffer.indices().head().store(head, Ordering::Release);
            Ok(value)
        } else {
            Err(PopError::Empty)
        }
    }

    /// Attempts to read an element from the queue without removing it.
    ///
    /// # Errors
    ///
    /// If the queue is empty, an error is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::{PeekError, RingBuffer};
    ///
    /// let (mut p, c) = RingBuffer::new(1);
    ///
    /// assert_eq!(c.peek(), Err(PeekError::Empty));
    /// assert_eq!(p.push(10), Ok(()));
    /// assert_eq!(c.peek(), Ok(&10));
    /// assert_eq!(c.peek(), Ok(&10));
    /// ```
    pub fn peek(&self) -> Result<&S::Item, PeekError> {
        if let Some(head) = self.next_head() {
            Ok(unsafe { &*self.buffer.slot_ptr(head) })
        } else {
            Err(PeekError::Empty)
        }
    }

    /// Returns the number of slots available for reading.
    ///
    /// Since items can be concurrently produced on another thread, the actual number
    /// of available slots may increase at any time (up to the [`RingBuffer::capacity()`]).
    ///
    /// To check for a single available slot,
    /// using [`Consumer::is_empty()`] is often quicker
    /// (because it might not have to check an atomic variable).
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (p, c) = RingBuffer::<f32>::new(1024);
    ///
    /// assert_eq!(c.slots(), 0);
    /// ```
    pub fn slots(&self) -> usize {
        let tail = self.buffer.indices().tail().load(Ordering::Acquire);
        self.cached_tail.set(tail);
        // "head" is only ever written by the consumer thread, "Relaxed" is enough
        let head = self.buffer.indices().head().load(Ordering::Relaxed);
        self.buffer.addr().distance(head, tail)
    }

    /// Returns `true` if there are currently no slots available for reading.
    ///
    /// An empty ring buffer might cease to be empty at any time
    /// if the corresponding [`Producer`] is producing items in another thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (p, c) = RingBuffer::<f32>::new(1);
    ///
    /// assert!(c.is_empty());
    /// ```
    ///
    /// Since items can be concurrently produced on another thread, the ring buffer
    /// might not be empty for long:
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (p, c) = RingBuffer::<f32>::new(1);
    /// if c.is_empty() {
    ///     // The buffer might be empty, but it might as well not be
    ///     // if an item was just produced on another thread.
    /// }
    /// ```
    ///
    /// However, if it's not empty, another thread cannot change that:
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (p, c) = RingBuffer::<f32>::new(1);
    /// if !c.is_empty() {
    ///     // At least one slot is guaranteed to be available for reading.
    /// }
    /// ```
    pub fn is_empty(&self) -> bool {
        self.next_head().is_none()
    }

    /*
    /// Returns `true` if the corresponding [`Producer`] has been destroyed.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (mut p, mut c) = RingBuffer::new(7);
    /// assert!(!c.is_abandoned());
    /// assert_eq!(p.push(10), Ok(()));
    /// drop(p);
    /// assert!(c.is_abandoned());
    /// // The items that are left in the ring buffer can still be consumed:
    /// assert_eq!(c.pop(), Ok(10));
    /// ```
    ///
    /// Since the producer can be concurrently dropped on another thread,
    /// the consumer might become abandoned at any time:
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (p, c) = RingBuffer::<i32>::new(1);
    /// if !c.is_abandoned() {
    ///     // Right now, the producer might still be alive, but it might as well not be
    ///     // if another thread has just dropped it.
    /// }
    /// ```
    ///
    /// However, if it already is abandoned, it will stay that way:
    ///
    /// ```
    /// # use rtrb::RingBuffer;
    /// # let (p, c) = RingBuffer::<i32>::new(1);
    /// if c.is_abandoned() {
    ///     // The producer does definitely not exist anymore.
    /// }
    /// ```
    pub fn is_abandoned(&self) -> bool {
        S::is_abandoned(&self.buffer)
    }
    */

    /// Returns the total capacity of the queue.
    ///
    /// At any time, the capacity is subdivided into
    /// [`Producer::slots()`] available for writing and
    /// [`Consumer::slots()`] available for reading.
    ///
    /// # Examples
    ///
    /// ```
    /// use rtrb::RingBuffer;
    ///
    /// let (producer, consumer) = RingBuffer::<f32>::new(100);
    /// assert_eq!(producer.capacity(), 100);
    /// assert_eq!(consumer.capacity(), 100);
    /// ```
    pub fn capacity(&self) -> usize {
        self.buffer.addr().capacity()
    }

    /// Get the head position for reading the next slot, if available.
    ///
    /// This is a strict subset of the functionality implemented in `read_chunk()`.
    /// For performance, this special case is immplemented separately.
    fn next_head(&self) -> Option<usize> {
        let indices = self.buffer.indices();
        // "head" is only ever written by the consumer thread, "Relaxed" is enough
        let head = indices.head().load(Ordering::Relaxed);

        // Check if the queue is *possibly* empty.
        if head == self.cached_tail.get() {
            // Refresh the tail ...
            let tail = indices.tail().load(Ordering::Acquire);
            // ... and check if it's *really* empty.
            if head == tail {
                return None;
            }
            self.cached_tail.set(tail);
        }
        Some(head)
    }
}

/// Error type for [`Consumer::pop()`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PopError {
    /// The queue was empty.
    Empty,
}

#[cfg(feature = "std")]
impl std::error::Error for PopError {}

impl fmt::Display for PopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PopError::Empty => "empty ring buffer".fmt(f),
        }
    }
}

/// Error type for [`Consumer::peek()`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeekError {
    /// The queue was empty.
    Empty,
}

#[cfg(feature = "std")]
impl std::error::Error for PeekError {}

impl fmt::Display for PeekError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeekError::Empty => "empty ring buffer".fmt(f),
        }
    }
}

/// Error type for [`Producer::push()`].
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum PushError<T> {
    /// The queue was full.
    Full(T),
}

#[cfg(feature = "std")]
impl<T> std::error::Error for PushError<T> {}

impl<T> fmt::Debug for PushError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PushError::Full(_) => f.pad("Full(_)"),
        }
    }
}

impl<T> fmt::Display for PushError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PushError::Full(_) => "full ring buffer".fmt(f),
        }
    }
}
