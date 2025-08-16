#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{TaskInfo, get_taskinfo};

fn test_taskinfo() {
    println!("=== Task Information Test ===");

    let task_info = get_taskinfo();

    println!("Current Task ID: {}", task_info.task_id);
    println!("Current Task Name: {}", task_info.get_name());

    // 显示原始字节数据（调试用）
    print!("Task name bytes: [");
    for (i, &byte) in task_info.task_name.iter().enumerate() {
        if byte == 0 {
            break;
        }
        if i > 0 {
            print!(", ");
        }
        print!("{}", byte);
    }
    println!("]");

    println!("=== End of Task Information ===");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("Task Info System Call Test");
    println!("===========================");

    // 多次调用测试
    for i in 1..=3 {
        println!("\n--- Test #{} ---", i);
        test_taskinfo();
    }

    println!("\nTask info test completed successfully!");
    0
}
