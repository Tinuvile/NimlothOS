#![no_std] // 禁用标准库
#![no_main] // 禁用入口函数
#![feature(custom_test_frameworks)]
#![test_runner(NimlothOS::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::{entry_point, BootInfo};
use NimlothOS::println;
use NimlothOS::task::{executor::Executor, keyboard, simple_executor::SimpleExecutor, Task};

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

    #[cfg(test)]
    test_main();

    let mut executor = Executor::new();
    executor.spawn(Task::new(example_task()));
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.run();

    println!("It did not crash!");

    NimlothOS::hlt_loop();
}

async fn async_number() -> u32 {
    42
}

async fn example_task() {
    let number = async_number().await;
    println!("number is {}", number);
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
