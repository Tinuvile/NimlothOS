#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::get_taskinfo;

fn test_comprehensive_stats() {
    println!("=== Comprehensive Statistics Test ===");

    let task_info = get_taskinfo();
    println!("Task ID: {}", task_info.task_id);
    println!("Task Name: {}", task_info.get_name());
    println!("Exit Code: {}", task_info.exit_code);
    println!("Status: {}", task_info.status);
    println!("Execution Time: {} cycles", task_info.get_execution_time());
    println!("Total System Calls: {}", task_info.get_total_calls());

    // 显示系统调用统计
    let syscall_names = [(64, "write"), (93, "exit"), (410, "get_taskinfo")];

    for (id, name) in &syscall_names {
        let count = task_info.get_syscall_count(*id);
        if count > 0 {
            println!("  {} ({}): {} times", name, id, count);
        }
    }

    println!("=== End of Test ===");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("Comprehensive Statistics Test");
    println!("=============================");

    // 多次调用系统调用来产生统计
    for i in 1..=3 {
        println!("\n--- Test Round #{} ---", i);
        let _info = get_taskinfo();
        test_comprehensive_stats();
    }

    println!("\nComprehensive test completed successfully!");
    0
}
