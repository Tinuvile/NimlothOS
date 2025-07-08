#![no_std]  // 禁用标准库
#![no_main]  // 禁用入口函数

#![feature(custom_test_frameworks)]
#![test_runner(NimlothOS::test_runner)]
#![reexport_test_harness_main = "test_main"]

use NimlothOS::{println};

#[unsafe(no_mangle)]  // 禁用名称重整
pub extern "C" fn _start() -> ! {  // 使用C语言的调用约定
    // 因为链接器会寻找一个名为_start的函数，所以这个函数就是入口点
    // 默认命名为_start

    println!("Hello World{}", "!");

    NimlothOS::init();

    // x86_64::instructions::interrupts::int3();

    unsafe {
        *(0xdeadbeef as *mut u8) = 42;
    };

    #[cfg(test)]
    test_main();

    println!("It did not crash! yeeeeee");

    loop{}
}


// 实现panic处理函数
use core::panic::PanicInfo;

#[cfg(not(test))]  // 在测试模式下不启用panic处理函数
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop{}
}

#[cfg(test)]  // 在测试模式下用serial_println替代println
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    NimlothOS::test_panic_handler(info)
}


#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}