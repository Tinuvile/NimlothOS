#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate user_lib;

use alloc::vec::Vec;
use user_lib::{exit, fork, time, wait};

/// 测试进程优先级降级的简单程序
/// 创建多个CPU密集型进程，观察它们的执行顺序和频率变化
fn cpu_bound_worker(worker_id: usize, work_amount: usize) {
    let start_time = time();
    let mut completed_work = 0;
    let mut last_report = start_time;

    println!("[Worker-{}] Started at {}ms", worker_id, start_time);

    for i in 0..work_amount {
        // 执行一些计算密集型工作
        let mut dummy = i;
        for _ in 0..1000 {
            dummy = (dummy * 1103515245 + 12345) & 0x7fffffff;
        }
        completed_work += 1;

        // 每完成10000单位工作报告一次
        if completed_work % 10000 == 0 {
            let current_time = time();
            let elapsed = current_time - last_report;
            println!(
                "[Worker-{}] Progress: {}/{} at {}ms (+{}ms)",
                worker_id, completed_work, work_amount, current_time, elapsed
            );
            last_report = current_time;
        }
    }

    let end_time = time();
    println!(
        "[Worker-{}] COMPLETED: {} work units in {}ms",
        worker_id,
        completed_work,
        end_time - start_time
    );
    exit(worker_id as i32);
}

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("=== Priority Demotion Test ===");
    println!("This test creates CPU-intensive processes to observe MLFQ behavior");
    println!("In MLFQ, processes should start with high priority and be demoted");
    println!("as they consume their time slices without yielding.");
    println!("");

    let test_start = time();
    let mut children = Vec::new();
    let work_per_process = 50000;

    // 创建4个CPU密集型工作进程
    for i in 0..4 {
        let pid = fork();
        if pid == 0 {
            cpu_bound_worker(i + 1, work_per_process);
        } else {
            children.push((pid, i + 1));
            println!("Created worker process {}: pid {}", i + 1, pid);
        }

        // 稍微延迟创建，这样可以观察到不同的启动时间
        if i < 3 {
            // 做一些轻微的延迟工作
            for _ in 0..10000 {
                let _ = time();
            }
        }
    }

    println!("\nAll worker processes created. Waiting for completion...");
    println!("Monitoring execution order (should show priority changes):");
    println!("");

    // 等待所有子进程完成
    for _ in 0..children.len() {
        let mut exit_code = 0;
        let finished_pid = wait(&mut exit_code);
        let current_time = time();

        // 找到对应的工作进程ID
        let worker_id = children
            .iter()
            .find(|(pid, _)| *pid == finished_pid)
            .map(|(_, id)| *id)
            .unwrap_or(0);

        println!(
            ">>> Worker {} (pid {}) finished at {}ms with exit code {}",
            worker_id,
            finished_pid,
            current_time - test_start,
            exit_code
        );
    }

    let test_end = time();
    println!("\n=== Test Complete ===");
    println!("Total duration: {}ms", test_end - test_start);
    println!("\nMLFQ Analysis:");
    println!("- Processes that started first should have been demoted faster");
    println!("- Later processes might have finished sooner due to higher priority");
    println!("- Check the timing patterns in the progress reports above");

    0
}
