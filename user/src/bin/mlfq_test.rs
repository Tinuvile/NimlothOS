#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate user_lib;

use alloc::vec::Vec;
use user_lib::{exit, fork, sleep, time, wait, yield_};

const TEST_DURATION: usize = 3000; // 3秒测试时间
const CPU_INTENSIVE_ITERATIONS: usize = 1000000;
const IO_SLEEP_TIME: usize = 10; // 10ms 睡眠模拟 I/O

/// CPU 密集型进程：大量计算，很少让出 CPU
fn cpu_intensive_process(process_id: usize) {
    let start_time = time();
    let mut counter = 0u64;
    let mut iterations = 0;

    println!("[CPU-{}] Started at time {}ms", process_id, start_time);

    while time() - start_time < TEST_DURATION as isize {
        // 执行大量计算
        for _ in 0..CPU_INTENSIVE_ITERATIONS {
            counter = counter.wrapping_add(1);
            counter = counter.wrapping_mul(3);
            counter = counter ^ 0x5555;
        }
        iterations += 1;

        // 偶尔报告状态
        if iterations % 10 == 0 {
            let current_time = time();
            println!(
                "[CPU-{}] Completed {} iterations at {}ms",
                process_id, iterations, current_time
            );
        }
    }

    let end_time = time();
    println!(
        "[CPU-{}] Finished: {} iterations in {}ms",
        process_id,
        iterations,
        end_time - start_time
    );
    exit(0);
}

/// I/O 密集型进程：频繁睡眠模拟 I/O 操作
fn io_intensive_process(process_id: usize) {
    let start_time = time();
    let mut io_operations = 0;

    println!("[IO-{}] Started at time {}ms", process_id, start_time);

    while time() - start_time < TEST_DURATION as isize {
        // 模拟 I/O 操作
        sleep(IO_SLEEP_TIME);
        io_operations += 1;

        // 做一些轻量级计算
        let mut dummy = 0u32;
        for _ in 0..1000 {
            dummy = dummy.wrapping_add(1);
        }

        // 报告状态
        if io_operations % 20 == 0 {
            let current_time = time();
            println!(
                "[IO-{}] Completed {} I/O ops at {}ms",
                process_id, io_operations, current_time
            );
        }
    }

    let end_time = time();
    println!(
        "[IO-{}] Finished: {} I/O operations in {}ms",
        process_id,
        io_operations,
        end_time - start_time
    );
    exit(0);
}

/// 混合型进程：交替进行计算和 I/O
fn mixed_process(process_id: usize) {
    let start_time = time();
    let mut cycles = 0;

    println!("[MIX-{}] Started at time {}ms", process_id, start_time);

    while time() - start_time < TEST_DURATION as isize {
        // CPU 阶段
        let mut counter = 0u64;
        for _ in 0..50000 {
            counter = counter.wrapping_add(1);
            counter = counter.wrapping_mul(7);
        }

        // I/O 阶段
        sleep(5);

        cycles += 1;

        if cycles % 50 == 0 {
            let current_time = time();
            println!(
                "[MIX-{}] Completed {} cycles at {}ms",
                process_id, cycles, current_time
            );
        }
    }

    let end_time = time();
    println!(
        "[MIX-{}] Finished: {} cycles in {}ms",
        process_id,
        cycles,
        end_time - start_time
    );
    exit(0);
}

/// 短任务进程：快速完成的进程
fn short_task_process(process_id: usize, task_size: usize) {
    let start_time = time();
    println!(
        "[SHORT-{}] Started short task at {}ms",
        process_id, start_time
    );

    // 执行一些快速计算
    let mut result = 0u64;
    for i in 0..task_size {
        result = result.wrapping_add(i as u64);
        if i % 10000 == 0 {
            yield_(); // 偶尔让出 CPU
        }
    }

    let end_time = time();
    println!(
        "[SHORT-{}] Finished in {}ms (result: {})",
        process_id,
        end_time - start_time,
        result % 1000
    );
    exit(0);
}

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("=== MLFQ Scheduler Test ===");
    println!("Testing Multi-Level Feedback Queue scheduling");
    println!("Expected behavior:");
    println!("- CPU-intensive processes should be demoted to lower priority queues");
    println!("- I/O-intensive processes should stay in higher priority queues");
    println!("- Short tasks should complete quickly with high priority");
    println!("- Mixed processes should get balanced treatment");
    println!("");

    let test_start = time();
    let mut children = Vec::new();

    // 创建2个 CPU 密集型进程
    for i in 0..2 {
        let pid = fork();
        if pid == 0 {
            cpu_intensive_process(i + 1);
        } else {
            children.push(pid);
            println!("Created CPU-intensive process {}: pid {}", i + 1, pid);
        }
    }

    // 等待一点时间，让CPU密集型进程开始运行
    sleep(100);

    // 创建2个 I/O 密集型进程
    for i in 0..2 {
        let pid = fork();
        if pid == 0 {
            io_intensive_process(i + 1);
        } else {
            children.push(pid);
            println!("Created I/O-intensive process {}: pid {}", i + 1, pid);
        }
    }

    // 创建1个混合型进程
    let pid = fork();
    if pid == 0 {
        mixed_process(1);
    } else {
        children.push(pid);
        println!("Created mixed process: pid {}", pid);
    }

    // 定期创建短任务来测试响应性
    for i in 0..5 {
        sleep(500); // 每500ms创建一个短任务
        let pid = fork();
        if pid == 0 {
            short_task_process(i + 1, 50000);
        } else {
            children.push(pid);
            println!(
                "Created short task {}: pid {} at {}ms",
                i + 1,
                pid,
                time() - test_start
            );
        }
    }

    // 等待所有子进程完成
    println!("\nWaiting for all processes to complete...");
    let mut exit_code = 0;
    for (i, _child_pid) in children.iter().enumerate() {
        let result_pid = wait(&mut exit_code);
        println!(
            "Process {} (pid {}) completed with exit code {}",
            i + 1,
            result_pid,
            exit_code
        );
    }

    let test_end = time();
    println!("\n=== MLFQ Test Complete ===");
    println!("Total test duration: {}ms", test_end - test_start);
    println!("\nExpected observations in MLFQ:");
    println!("1. Short tasks should have completed quickly");
    println!("2. I/O processes should have had frequent execution");
    println!("3. CPU processes should have been demoted and run less frequently");
    println!("4. Mixed processes should have balanced execution");

    0
}
