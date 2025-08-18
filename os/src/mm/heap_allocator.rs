use core::ptr::addr_of_mut;

use crate::config::KERNEL_HEAP_SIZE;
use buddy_system_allocator::LockedHeap;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::empty(); //2^32 = 4GB

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(
            addr_of_mut!(HEAP_SPACE) as *mut u8 as usize,
            KERNEL_HEAP_SIZE,
        );
    }
}

#[alloc_error_handler]
pub fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}
