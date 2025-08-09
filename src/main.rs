#![no_std] // 禁用标准库
#![no_main] // 禁用入口函数
#![feature(custom_test_frameworks)]
#![test_runner(NimlothOS::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::{entry_point, BootInfo};
use NimlothOS::println;

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use x86_64::VirtAddr;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use NimlothOS::allocator;
    use NimlothOS::memory::{self, BootInfoFrameAllocator};

    println!("Hello World{}", "!");

    NimlothOS::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("Heap initialization failed");

    let heap_value = Box::new(41);
    println!("heap_value at {:p}", heap_value);

    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    println!("vec at {:p}", vec.as_slice());

    let reference_counted = Rc::new(vec![1, 2, 3]);
    let clone_reference = reference_counted.clone();
    println!(
        "current reference count is {}",
        Rc::strong_count(&reference_counted)
    );
    core::mem::drop(clone_reference);
    println!(
        "reference count is {} now",
        Rc::strong_count(&reference_counted)
    );

    #[cfg(test)]
    test_main();

    println!("It did not crash!");

    NimlothOS::hlt_loop();
}

// 实现panic处理函数
use core::panic::PanicInfo;

#[cfg(not(test))] // 在测试模式下不启用panic处理函数
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    NimlothOS::hlt_loop();
}

#[cfg(test)] // 在测试模式下用serial_println替代println
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    NimlothOS::test_panic_handler(info)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
