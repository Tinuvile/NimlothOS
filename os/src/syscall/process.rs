//! # 进程管理相关系统调用
//!
//! 实现与进程生命周期管理相关的系统调用，包括进程退出、
//! CPU 时间片让出和系统时间获取等功能。

use crate::fs::{OpenFlags, open_file};
use crate::mm::{translated_refmut, translated_str};
use crate::println;
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next,
};
use crate::timer::time_ms;
use alloc::sync::Arc;

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
    exit_current_and_run_next(exit_code);
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

/// 获取系统时间系统调用
///
/// 返回系统启动以来的毫秒数，用于用户程序的时间测量。
///
/// ## Returns
///
/// 系统启动以来的毫秒数
///
/// ## Examples
///
/// ```c
/// long start_time = time();
/// // ... 执行一些操作 ...
/// long end_time = time();
/// long elapsed = end_time - start_time;
/// ```
pub fn sys_time() -> isize {
    time_ms() as isize
}

/// 系统调用：获取进程 PID
///
/// 返回当前进程的进程标识符（PID）。
/// - 成功时返回一个非负整数 PID。
/// - 本实现中不会失败。
pub fn sys_pid() -> isize {
    current_task().unwrap().pid.0 as isize
}

/// 系统调用：创建子进程（fork）
///
/// 实现 `fork(2)` 系统调用，创建当前进程的一个子进程。子进程将
/// 拥有与父进程相同的地址空间内容（独立复制），并从 `fork` 返回处
/// 开始执行。父进程得到子进程的 PID，子进程得到返回值 0。
///
/// ## Returns
///
/// - 父进程中返回新建子进程的 PID（正数）
/// - 子进程中返回 0
///
/// ## 行为说明
///
/// 1. 复制父进程的任务控制块及地址空间（深拷贝）
/// 2. 设置子进程 Trap 上下文的返回值 `a0 = 0`
/// 3. 将子进程加入就绪队列等待调度
///
/// ## 安全与隔离
///
/// - 子进程拥有独立的物理页面副本，修改互不影响
/// - Trampoline 等只读共享页面除外
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    let trap_cx = new_task.inner_exclusive_access().trap_cx();
    trap_cx.x[10] = 0; // x[10] = a0
    add_task(new_task);
    new_pid as isize
}

/// 系统调用：执行新程序（exec）
///
/// 实现 `execve(2)` 的简化版本，用指定的程序镜像替换当前进程的
/// 地址空间。若加载成功，不返回；若失败，返回 -1。
///
/// ## Arguments
///
/// * `path` - 指向用户空间以 `\0` 结尾的程序名字符串
///
/// ## Returns
///
/// - 成功时返回 0（实际执行不会返回到此处，进程上下文被替换）
/// - 失败时返回 -1（未找到指定程序）
///
/// ## 行为说明
///
/// 1. 从用户态读取程序名字符串
/// 2. 打开文件
/// 3. 读取文件内容
/// 4. 调用任务的 `exec` 方法重建地址空间并跳转到新入口
pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = data.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// 系统调用：等待子进程结束（waitpid）
///
/// 实现 `waitpid(2)` 的子集，等待特定 PID 的子进程或任意子进程结束，
/// 并将其退出码写入到用户提供的缓冲区。
///
/// ## Arguments
///
/// * `pid` - 要等待的子进程 PID；传入 `-1` 表示等待任意子进程
/// * `exit_code_ptr` - 指向用户空间的退出码写入地址
///
/// ## Returns
///
/// - 成功时返回已回收子进程的 PID
/// - 若没有匹配的子进程返回 -1
/// - 若暂时没有已退出的符合条件的子进程返回 -2（可由上层重试/阻塞）
///
/// ## 行为说明
///
/// 1. 校验待等待的子进程是否存在
/// 2. 查找符合条件且已处于 Zombie 状态的子进程
/// 3. 回收其 Task 对象，获取退出码并写回用户缓冲区
///
/// ## Safety
///
/// 通过 `translated_refmut()` 将退出码写入用户空间，调用前已验证指针
/// 在当前地址空间内有效（失败会 panic；未来可改为错误返回）。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        let exit_code = child.inner_exclusive_access().exit_code;
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
}
