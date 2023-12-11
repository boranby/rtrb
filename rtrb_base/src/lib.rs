#![no_std]
#![warn(rust_2018_idioms)]

use core::sync::atomic::{AtomicUsize, Ordering};

pub trait Indices {
    fn new() -> Self;

    fn head(&self) -> &AtomicUsize;
    fn tail(&self) -> &AtomicUsize;
}

// TODO: unsafe trait? or make all methods unsafe?
pub trait Addressing {
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

// TODO: unsafe trait? or make all methods unsafe?
// Safety: Storage must be contiguous.
pub trait Storage {
    type Item;
    type Addr: Addressing;
    type Indices: Indices;

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
}
