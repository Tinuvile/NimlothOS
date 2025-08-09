#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(NimlothOS::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(main);

fn main(_boot_info: &'static BootInfo) -> ! {
    use NimlothOS::allocator;
    use NimlothOS::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    NimlothOS::init();

    let phys_mem_offset = VirtAddr::new(_boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { 
        BootInfoFrameAllocator::init(&_boot_info.memory_map) 
    };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("Heap initialization failed");

    test_main();

    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    NimlothOS::test_panic_handler(_info);
}

use alloc::boxed::Box;

#[test_case]
fn simple_allocation() {
    let heap_value_1 = Box::new(41);
    let heap_value_2 = Box::new(13);

    assert_eq!(*heap_value_1, 41);
    assert_eq!(*heap_value_2, 13);
}

use alloc::vec::Vec;

#[test_case]
fn large_vec() {
    let n = 1000;
    let mut vec = Vec::new();
    for i in 0..n {
        vec.push(i);
    }
    assert_eq!(vec.iter().sum::<u64>(), (n - 1) * n / 2);
}

use NimlothOS::allocator::HEAP_SIZE;

#[test_case]
fn many_boxes() {
    for i in 0..HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }
}