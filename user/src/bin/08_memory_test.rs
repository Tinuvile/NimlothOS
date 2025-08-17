#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{exit, write};

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("Memory boundary test starting...");

    // 测试1：正常写入（应该成功）
    let valid_str = "This is a valid string\n";
    let result1 = write(1, valid_str.as_bytes());
    println!("Test 1 (valid buffer): result = {}", result1);

    // 测试2：尝试写入内核内存（应该失败）
    let kernel_addr = 0x80200000 as *const u8; // 内核地址
    let result2 = write(1, unsafe { core::slice::from_raw_parts(kernel_addr, 10) });
    println!("Test 2 (kernel address): result = {}", result2);

    // 测试3：尝试写入超出用户空间的地址（应该失败）
    let invalid_addr = 0x90000000 as *const u8; // 超出用户空间
    let result3 = write(1, unsafe { core::slice::from_raw_parts(invalid_addr, 10) });
    println!("Test 3 (out of bounds): result = {}", result3);

    // 测试4：尝试写入长度导致越界（应该失败）
    let valid_addr = 0x80400000 as *const u8; // 用户空间起始
    let huge_len = 0x30000; // 超过APP_SIZE_LIMIT的长度
    let result4 = write(1, unsafe {
        core::slice::from_raw_parts(valid_addr, huge_len)
    });
    println!("Test 4 (length overflow): result = {}", result4);

    println!("Memory boundary test completed!");
    0
}
