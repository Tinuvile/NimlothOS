//! # 进程管理相关系统调用
//!
//! 实现与进程生命周期管理相关的系统调用，包括进程退出、
//! CPU 时间片让出和系统时间获取等功能。

use crate::println;
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next};
use crate::timer::get_time_ms;

/// 系统调用：进程退出
///
/// 实现 `exit(2)` 系统调用，终止当前正在运行的应用程序。
/// 该函数会清理当前任务的资源，并调度下一个就绪任务运行。
///
/// ## Arguments
///
/// * `exit_code` - 进程退出码，用于向父进程或系统报告执行状态
///   - 0 通常表示成功完成
///   - 非零值通常表示出现错误或异常情况
///
/// ## Returns
///
/// 该函数实际上不会返回，因为调用进程会被终止。
/// 返回类型 `isize` 仅用于与系统调用接口保持一致。
///
/// ## Behavior
///
/// 1. 记录进程退出信息到内核日志
/// 2. 将当前任务标记为已退出状态
/// 3. 调度并切换到下一个就绪任务
/// 4. 如果没有其他任务，系统将处理所有任务完成的情况
///
/// ## Panics
///
/// 如果任务切换函数意外返回，会触发 panic（这在正常情况下不应该发生）
///
/// ## Examples
///
/// 从用户态调用：
/// ```c
/// exit(0);  // 正常退出
/// exit(1);  // 异常退出
/// ```
pub fn sys_exit(exit_code: i32) -> isize {
    println!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// 系统调用：让出 CPU 时间片
///
/// 实现 `sched_yield(2)` 系统调用，当前任务主动让出 CPU，
/// 允许调度器选择其他就绪任务运行。这是一种协作式多任务的实现方式。
///
/// ## Returns
///
/// 成功时返回 0。当任务重新获得 CPU 时间片时，会从此处继续执行。
///
/// ## Behavior
///
/// 1. 将当前任务状态从 `Running` 改为 `Ready`
/// 2. 调度器选择下一个就绪任务运行
/// 3. 执行任务上下文切换
/// 4. 当前任务将来重新被调度时，从此函数返回
///
/// ## Use Cases
///
/// - 长时间运行的任务主动让出 CPU，提高系统响应性
/// - 等待某些条件满足时的忙等待优化
/// - 实现用户空间的协作式多任务
///
/// ## Examples
///
/// 从用户态调用：
/// ```c
/// while (condition) {
///     yield();  // 避免长时间占用 CPU
/// }
/// ```
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

/// 系统调用：获取系统时间
///
/// 实现获取系统运行时间的系统调用，返回自系统启动以来经过的毫秒数。
/// 这是 `gettimeofday(2)` 的简化版本。
///
/// ## Returns
///
/// 返回系统启动以来经过的毫秒数（作为 `isize` 类型）
///
/// ## Precision
///
/// 时间精度取决于系统时钟频率和定时器实现，通常为毫秒级精度。
///
/// ## Use Cases
///
/// - 测量代码执行时间
/// - 实现定时器和超时机制
/// - 性能分析和基准测试
/// - 随机数生成的种子
///
/// ## Examples
///
/// 从用户态调用：
/// ```c
/// long start_time = get_time();
/// // 执行某些操作
/// long end_time = get_time();
/// long elapsed = end_time - start_time;  // 计算耗时
/// ```
///
/// ## Note
///
/// 返回的时间值可能会在长时间运行后溢出，调用者应该考虑处理这种情况。
pub fn sys_get_time() -> isize {
    get_time_ms() as isize
}
