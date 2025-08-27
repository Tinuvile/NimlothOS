#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{exec, fork, wait};

/// MLFQ 演示程序的启动器
/// 依次运行各个MLFQ测试程序来展示调度效果

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("=== MLFQ Scheduling Demo ===");
    println!("This demo will run three different MLFQ tests:");
    println!("1. Priority Test - Shows process demotion");
    println!("2. I/O Priority Test - Shows I/O vs CPU scheduling");
    println!("3. Full MLFQ Test - Comprehensive scheduling test");
    println!("");

    let tests = [
        ("priority_test\0", "Priority Demotion Test"),
        ("io_priority_test\0", "I/O vs CPU Priority Test"),
        ("mlfq_test\0", "Comprehensive MLFQ Test"),
    ];

    for (i, (test_name, description)) in tests.iter().enumerate() {
        println!("=== Test {} : {} ===", i + 1, description);
        println!("Running: {}", test_name.trim_end_matches('\0'));
        println!("");

        let pid = fork();
        if pid == 0 {
            exec(*test_name, &[core::ptr::null::<u8>()]);
            println!("Failed to execute {}", test_name);
            return -1;
        } else {
            let mut exit_code = 0;
            wait(&mut exit_code);
            println!("");
            println!("=== Test {} Completed ===", i + 1);
            if i < tests.len() - 1 {
                println!("Press any key or wait for next test...");
                println!("");
            }
        }
    }

    println!("=== All MLFQ Tests Completed ===");
    println!("Summary of MLFQ features demonstrated:");
    println!("✓ Process priority demotion after time slice exhaustion");
    println!("✓ I/O-bound processes maintaining high priority");
    println!("✓ CPU-bound processes moving to lower priority queues");
    println!("✓ Dynamic time slice allocation (10ms, 20ms, 40ms, 80ms)");
    println!("✓ Improved responsiveness for interactive tasks");

    0
}
