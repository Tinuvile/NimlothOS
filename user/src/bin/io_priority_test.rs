#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate user_lib;

use alloc::vec::Vec;
use user_lib::{exit, fork, sleep, time, wait};

/// CPU密集型进程，应该被降级
fn cpu_hog(process_id: usize) {
    let start_time = time();
    let mut iterations = 0;

    println!("[CPU-HOG-{}] Started at {}ms", process_id, start_time);

    // 持续运行2秒
    while time() - start_time < 2000 {
        // 大量计算，不让出CPU
        let mut counter = 0u64;
        for _ in 0..500000 {
            counter = counter.wrapping_mul(1103515245).wrapping_add(12345);
        }
        iterations += 1;

        if iterations % 100 == 0 {
            println!(
                "[CPU-HOG-{}] Iteration {} at {}ms",
                process_id,
                iterations,
                time() - start_time
            );
        }
    }

    let end_time = time();
    println!(
        "[CPU-HOG-{}] Finished {} iterations in {}ms",
        process_id,
        iterations,
        end_time - start_time
    );
    exit(0);
}

/// I/O密集型进程，应该保持高优先级
fn io_worker(process_id: usize) {
    let start_time = time();
    let mut io_count = 0;

    println!("[IO-WORKER-{}] Started at {}ms", process_id, start_time);

    // 运行2秒
    while time() - start_time < 2000 {
        // 模拟I/O操作（睡眠）
        sleep(20);
        io_count += 1;

        // 做一些轻量计算
        let mut _result = 0;
        for i in 0..1000 {
            _result += i;
        }

        println!(
            "[IO-WORKER-{}] I/O operation {} completed at {}ms",
            process_id,
            io_count,
            time() - start_time
        );
    }

    let end_time = time();
    println!(
        "[IO-WORKER-{}] Finished {} I/O operations in {}ms",
        process_id,
        io_count,
        end_time - start_time
    );
    exit(0);
}

/// 间歇性任务，测试优先级恢复
fn bursty_worker(process_id: usize) {
    let start_time = time();
    let mut burst_count = 0;

    println!("[BURSTY-{}] Started at {}ms", process_id, start_time);

    while time() - start_time < 2500 {
        // CPU突发阶段
        println!(
            "[BURSTY-{}] Starting burst {} at {}ms",
            process_id,
            burst_count + 1,
            time() - start_time
        );

        let burst_start = time();
        let mut work_done = 0;
        while time() - burst_start < 100 {
            // 100ms CPU突发
            let mut dummy = 0u64;
            for _ in 0..10000 {
                dummy = dummy.wrapping_add(1);
            }
            work_done += 1;
        }

        burst_count += 1;
        println!(
            "[BURSTY-{}] Burst {} finished ({} work units) at {}ms",
            process_id,
            burst_count,
            work_done,
            time() - start_time
        );

        // 空闲阶段（模拟等待外部事件）
        sleep(200);
        println!(
            "[BURSTY-{}] Idle period finished at {}ms",
            process_id,
            time() - start_time
        );
    }

    let end_time = time();
    println!(
        "[BURSTY-{}] Finished {} bursts in {}ms",
        process_id,
        burst_count,
        end_time - start_time
    );
    exit(0);
}

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("=== I/O vs CPU Priority Test ===");
    println!("This test demonstrates MLFQ's I/O optimization:");
    println!("- CPU-intensive processes get demoted to lower priority");
    println!("- I/O-intensive processes maintain high priority");
    println!("- Bursty processes show priority recovery after I/O");
    println!("");

    let test_start = time();
    let mut children = Vec::new();

    // 先启动一个CPU密集型进程，让它开始被降级
    let pid = fork();
    if pid == 0 {
        cpu_hog(1);
    } else {
        children.push(pid);
        println!("Started CPU-intensive process: pid {}", pid);
    }

    // 等待500ms，让CPU进程被降级
    sleep(500);

    // 启动I/O密集型进程，它应该立即获得高优先级
    let pid = fork();
    if pid == 0 {
        io_worker(1);
    } else {
        children.push(pid);
        println!("Started I/O-intensive process: pid {}", pid);
    }

    // 等待200ms
    sleep(200);

    // 启动另一个CPU密集型进程
    let pid = fork();
    if pid == 0 {
        cpu_hog(2);
    } else {
        children.push(pid);
        println!("Started second CPU-intensive process: pid {}", pid);
    }

    // 等待300ms
    sleep(300);

    // 启动间歇性进程
    let pid = fork();
    if pid == 0 {
        bursty_worker(1);
    } else {
        children.push(pid);
        println!("Started bursty process: pid {}", pid);
    }

    // 启动第二个I/O进程
    let pid = fork();
    if pid == 0 {
        io_worker(2);
    } else {
        children.push(pid);
        println!("Started second I/O-intensive process: pid {}", pid);
    }

    println!("\nAll processes started. Monitoring execution...");
    println!("Expected behavior:");
    println!("- I/O workers should get frequent CPU time");
    println!("- CPU hogs should get progressively less CPU time");
    println!("- Bursty worker should recover priority after each sleep");
    println!("");

    // 等待所有子进程完成
    for _i in 0..children.len() {
        let mut exit_code = 0;
        let finished_pid = wait(&mut exit_code);
        let elapsed = time() - test_start;
        println!(">>> Process {} finished at {}ms", finished_pid, elapsed);
    }

    let test_end = time();
    println!("\n=== I/O Priority Test Complete ===");
    println!("Total duration: {}ms", test_end - test_start);
    println!("\nAnalysis:");
    println!("- I/O processes should have had consistent response times");
    println!("- CPU processes should show declining performance over time");
    println!("- Check the timing patterns to verify MLFQ behavior");

    0
}
