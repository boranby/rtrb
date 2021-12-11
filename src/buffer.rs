use core::cell::Cell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use cache_padded::CachePadded;

#[repr(C)]
struct RingBuffer<T> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    is_abandoned: AtomicBool,
    /// Indicates that dropping a `RingBuffer<T>` may drop elements of type `T`.
    _marker: PhantomData<T>,
    /// Dynamically sized field must be the last one
    // TODO: UnsafeCell?
    slots: [MaybeUninit<T>],
}

impl<T> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        // TODO: actual implementation

        println!("Dropping RingBuffer!");
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Producer<T> {
    buffer: NonNull<RingBuffer<T>>,
    head: Cell<usize>,
    tail: Cell<usize>,
}

unsafe fn abandon<T>(buffer: NonNull<RingBuffer<T>>) {
    // We don't care about the ordering of other reads or writes, Relaxed is enough.
    if buffer
        .as_ref()
        .is_abandoned
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        Box::from_raw(buffer.as_ptr());
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        println!("Dropping Producer!");
        unsafe { abandon(self.buffer) };
    }
}

impl<T> Producer<T> {
    fn buffer(&self) -> &RingBuffer<T> {
        unsafe { self.buffer.as_ref() }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Consumer<T> {
    buffer: NonNull<RingBuffer<T>>,
    head: Cell<usize>,
    tail: Cell<usize>,
}

impl<T> Drop for Consumer<T> {
    fn drop(&mut self) {
        println!("Dropping Consumer!");
        unsafe { abandon(self.buffer) };
    }
}

fn try_to_create_dst<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    assert_ne!(
        std::mem::size_of::<T>(),
        0,
        "TODO: check if this works with ZST"
    );

    use alloc::alloc::Layout;
    let layout = Layout::new::<()>();
    let (layout, head_offset) = layout
        .extend(Layout::new::<CachePadded<AtomicUsize>>())
        .unwrap();
    assert_eq!(head_offset, 0);
    let (layout, tail_offset) = layout
        .extend(Layout::new::<CachePadded<AtomicUsize>>())
        .unwrap();
    let (layout, is_abandoned_offset) = layout.extend(Layout::new::<AtomicBool>()).unwrap();
    let (layout, _slots_offset) = layout
        .extend(
            Layout::from_size_align(
                std::mem::size_of::<T>() * capacity,
                std::mem::align_of::<T>(),
            )
            .unwrap(),
        )
        .unwrap();
    let layout = layout.pad_to_align();

    let ptr = unsafe {
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            alloc::alloc::handle_alloc_error(layout);
        }
        ptr.add(head_offset)
            .cast::<CachePadded<AtomicUsize>>()
            .write(CachePadded::new(AtomicUsize::new(0)));
        ptr.add(tail_offset)
            .cast::<CachePadded<AtomicUsize>>()
            .write(CachePadded::new(AtomicUsize::new(0)));
        ptr.add(is_abandoned_offset)
            .cast::<AtomicBool>()
            .write(AtomicBool::new(false));
        let ptr = std::ptr::slice_from_raw_parts_mut(ptr, capacity) as *mut RingBuffer<T>;
        // Safety: null check has been done above
        NonNull::new_unchecked(ptr)
    };

    let p = Producer {
        buffer: ptr,
        head: Cell::new(0),
        tail: Cell::new(0),
    };
    let c = Consumer {
        buffer: ptr,
        head: Cell::new(0),
        tail: Cell::new(0),
    };
    (p, c)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn create_dst() {
        let (p, c) = try_to_create_dst::<u8>(22);

        let buf = p.buffer();
        assert_eq!(std::mem::size_of_val(&buf), std::mem::size_of::<&[u8]>());
    }
}
