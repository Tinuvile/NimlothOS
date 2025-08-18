#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::config::MAX_SYSCALL_COUNT;
use user_lib::task::{SyscallInfo, TaskInfo, TaskStatus};
use user_lib::{get_time, task_info, yield_};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let current_timer = get_time();
    let wait_for = current_timer + 3000;
    while get_time() < wait_for {
        yield_();
    }
    println!("Test sleep OK!");

    let ret = user_lib::task_info(0, core::ptr::null_mut());
    println!("task_info returned: {}", ret);

    0
}
