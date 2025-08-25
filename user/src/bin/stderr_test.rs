#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{err_print, err_println, write, write_stderr};

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("Testing standard error output...");

    // 直接写入标准错误
    let error_msg = b"Direct write to stderr: This is an error message\n";
    let result = write(2, error_msg);
    if result == -1 {
        println!("Failed to write to stderr");
        return -1;
    }

    // 使用 write_stderr 函数
    let error_msg2 = b"Using write_stderr function: Another error message\n";
    let result2 = write_stderr(error_msg2);
    if result2 == -1 {
        println!("Failed to write to stderr using write_stderr");
        return -1;
    }

    // 使用 err_print 函数
    let result3 = err_print("Using eprint function: Error message without newline");
    if result3 == -1 {
        println!("Failed to write to stderr using eprint");
        return -1;
    }

    // 使用 err_println 函数
    let result4 = err_println("Using eprintln function: Error message with newline");
    if result4 == -1 {
        println!("Failed to write to stderr using eprintln");
        return -1;
    }

    println!("Standard error output test completed successfully!");
    0
}
