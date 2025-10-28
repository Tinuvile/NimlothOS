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

/// CPU 密集型进程：大量计算，很少让出 CPU（静默版本）
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

        // 大幅减少报告频率：每1000次迭代报告一次
        if iterations % 1000 == 0 {
            let current_time = time();
            println!(
                "[CPU-{}] Progress: {} iterations at {}ms",
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

/// I/O 密集型进程：频繁睡眠模拟 I/O 操作（静默版本）
fn io_intensive_process(process_id: usize) {
    let start_time = time();
    let mut io_operations = 0;

    println!("[IO-{}] Started at time {}ms", process_id, start_time);

    while time() - start_time < TEST_DURATION as isize {
        // 模拟 I/O 操作
        sleep(IO_SLEEP_TIME);
        io_operations += 1;

        // 减少报告频率：每50次操作报告一次
        if io_operations % 50 == 0 {
            let current_time = time();
            println!(
                "[IO-{}] Progress: {} I/O ops at {}ms",
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

/// 混合型进程：交替进行计算和 I/O（静默版本）
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

        // 减少报告频率：每100个周期报告一次
        if cycles % 100 == 0 {
            let current_time = time();
            println!(
                "[MIX-{}] Progress: {} cycles at {}ms",
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
    println!("=== MLFQ Scheduler Test (Quiet Version) ===");
    println!("Reduced output for better readability");
    println!("Focus on SHORT tasks and final results");
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
    println!("\n=== Creating Short Tasks ===");
    for i in 0..3 {
        // 减少短任务数量
        sleep(800); // 每800ms创建一个短任务
        let pid = fork();
        if pid == 0 {
            short_task_process(i + 1, 50000);
        } else {
            children.push(pid);
            println!(
                ">>> Created SHORT TASK {}: pid {} at {}ms <<<",
                i + 1,
                pid,
                time() - test_start
            );
        }
    }

    // 等待所有子进程完成
    println!("\n=== Process Completion Status ===");
    let mut exit_code = 0;
    for (i, _child_pid) in children.iter().enumerate() {
        let result_pid = wait(&mut exit_code);
        println!(
            ">>> Process {} (pid {}) completed with exit code {} <<<",
            i + 1,
            result_pid,
            exit_code
        );
    }

    let test_end = time();
    println!("\n=== MLFQ Test Complete ===");
    println!("Total test duration: {}ms", test_end - test_start);
    println!("\nKey observations:");
    println!("1. Check if SHORT tasks completed quickly");
    println!("2. I/O processes should have frequent execution");
    println!("3. CPU processes should show declining performance");

    0
}
