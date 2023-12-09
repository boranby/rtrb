

trait Addressing {
    //type SizeType;
    fn capacity() -> usize;
    fn collapse_position(&self, pos: usize) -> usize;
}

trait Storage {
    type Item;
    type Addr: Addressing;
    
    fn data_ptr(&self) -> *mut Self::Item;
    
    fn addr(&self) -> &Self::Addr;
    
    unsafe fn slot_ptr(&self, pos: usize) -> *mut Self::Item {
        self.data_ptr().add(self.addr().collapse_position(pos))
    }
}

struct Buffer<S: ?Sized> {
    storage: S,
}

impl<S: Storage> Buffer<S> {
    fn bla(&self) -> *mut S::Item {
        self.storage.data_ptr()
    }
}

struct CachePaddedIndices {
    head: usize, // TODO: atomic, padded
}

struct PowerOfTwoAddressing {}
impl Addressing for PowerOfTwoAddressing {
    fn capacity() -> usize { 4 }
    fn collapse_position(&self, pos: usize) -> usize { pos }
}

struct TightAddressing {}
impl Addressing for TightAddressing {
    fn capacity() -> usize { 2 }
    fn collapse_position(&self, pos: usize) -> usize { pos + 1 }
}

struct DynamicStorage<T, A> {
    //cap: usize,
    data: Vec<T>,
    addr: A,
}

impl<T, A: Addressing> Storage for DynamicStorage<T, A> {
    type Item = T;
    type Addr = A;
    
    fn addr(&self) -> &Self::Addr { &self.addr }
    fn data_ptr(&self) -> *mut Self::Item { std::ptr::null_mut() }
}

type MyBuffer<T> = Buffer<DynamicStorage<T, PowerOfTwoAddressing>>;
type MyOtherBuffer<T> = Buffer<DynamicStorage<T, TightAddressing>>;

impl<T> MyBuffer<T> {
    fn new() -> Self {
        MyBuffer {
            storage: DynamicStorage::<T, PowerOfTwoAddressing>{
                data: Vec::new(),
                addr: PowerOfTwoAddressing{},
            },
        }
    }
}

fn main() {
    let b = MyBuffer::<f32>::new();
}
