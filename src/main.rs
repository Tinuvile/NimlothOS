#![no_std]  // 禁用标准库
#![no_main]  // 禁用入口函数

#[unsafe(no_mangle)]  // 禁用名称重整
pub extern "C" fn _start() -> !{  // 使用C语言的调用约定
    // 因为链接器会寻找一个名为_start的函数，所以这个函数就是入口点
    // 默认命名为_start
    loop{}
}

// 实现panic处理函数
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info:&PanicInfo)->!{
    loop{}
}

