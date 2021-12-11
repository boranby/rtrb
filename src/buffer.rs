use core::cell::Cell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize};

use cache_padded::CachePadded;

struct ProducerHeader {
    tail: AtomicUsize,
    /*
    // this should only be accessed from the producer thread
    capacity: usize,
    */
    is_alive: AtomicBool,
}

struct ConsumerHeader {
    head: AtomicUsize,
    /*
    capacity: usize,
    */
    is_alive: AtomicBool,
}

/*
#[repr(C)]
struct ArcInner<T: ?Sized> {
    pub(crate) count: atomic::AtomicUsize,
    pub(crate) data: T,
}
*/

/*
#[repr(C)]
struct Slots<T: ?Sized> {
    data: T,
}
*/

// dynamically sized type
#[repr(C)]
struct RingBuffer<T> {
    producer_header: CachePadded<ProducerHeader>,
    consumer_header: CachePadded<ConsumerHeader>,
    /// Indicates that dropping a `RingBuffer<T>` may drop elements of type `T`.
    _marker: PhantomData<T>,
    /// Dynamically sized field must be the last one
    // TODO: UnsafeCell?
    slots: [MaybeUninit<T>],
    //slots: Slots<[MaybeUninit<T>]>,
}

/*
#[repr(transparent)]
struct Pointer<T> {
    ptr: std::ptr::NonNull<RingBuffer<T>>,
    _marker: PhantomData<T>,
}
*/

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

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        println!("Dropping Producer!");
        let buf = self.buffer.as_ref();

        // TODO: if consumer is dead, deallocate

        // TODO set self to dead

        // TODO: if consumer was alive, check again, deallocate

        buf.producer_header.is_alive;
        buf.consumer_header.is_alive;
        unsafe { Box::from_raw(self.buffer.as_ptr()) };
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
        let buf = self.buffer.as_ref();

        // TODO: if producer is dead, deallocate

        // TODO set self to dead

        // TODO: if producer was alive, check again, deallocate

fn try_to_create_dst<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    assert_ne!(
        std::mem::size_of::<T>(),
        0,
        "TODO: check if this works with ZST"
    );

    #[repr(C)]
    struct Dummy<T> {
        producer_header: CachePadded<ProducerHeader>,
        consumer_header: CachePadded<ConsumerHeader>,
        //nonsense: bool,
        slots: [T; 0],
    }

    //let slice_offset = ???;

    println!(
        "producer size: {}",
        std::mem::size_of::<CachePadded<ProducerHeader>>()
    );
    println!(
        "consumer size: {}",
        std::mem::size_of::<CachePadded<ConsumerHeader>>()
    );
    println!("dummy size: {}", std::mem::size_of::<Dummy<T>>());

    //let ptr: *mut RingBuffer<T>;

    // TODO: get (uninitialized) buffer from somewhere

    // ptr.align_offset(align_of::<???>())

    use alloc::alloc::Layout;

    let producer_layout = Layout::new::<CachePadded<ProducerHeader>>();
    let consumer_layout = Layout::new::<CachePadded<ConsumerHeader>>();
    let slots_layout = Layout::from_size_align(
        std::mem::size_of::<T>() * capacity,
        std::mem::align_of::<T>(),
    )
    .unwrap();

    let layout = Layout::new::<()>();
    let (layout, p_header_offset) = layout.extend(producer_layout).unwrap();
    assert_eq!(p_header_offset, 0);
    let (layout, c_header_offset) = layout.extend(consumer_layout).unwrap();
    let (layout, _slots_offset) = layout.extend(slots_layout).unwrap();
    let layout = layout.pad_to_align();

    let ptr = unsafe {
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            alloc::alloc::handle_alloc_error(layout);
            unreachable!();
        }
        ptr.add(p_header_offset)
            .cast::<CachePadded<ProducerHeader>>()
            .write(CachePadded::new(ProducerHeader {
                tail: AtomicUsize::new(0),
                is_alive: AtomicBool::new(true),
            }));
        ptr.add(c_header_offset)
            .cast::<CachePadded<ConsumerHeader>>()
            .write(CachePadded::new(ConsumerHeader {
                head: AtomicUsize::new(0),
                is_alive: AtomicBool::new(true),
            }));
        let ptr = std::ptr::slice_from_raw_parts_mut(ptr, capacity) as *mut RingBuffer<T>;
        //assert_eq!(std::mem::size_of::<&RingBuffer<T>>(), std::mem::size_of::<&[T]>());
        assert_eq!((*ptr).slots.len(), capacity);
        // Safety: null check has been done above
        NonNull::new_unchecked(ptr)
    };
    //let reference: &RingBuffer<T> = ptr.as_ref();

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
