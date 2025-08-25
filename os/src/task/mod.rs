//! # 任务管理模块
//!
//! 提供内核中的任务/进程管理与调度功能，涵盖任务上下文保存与切换、
//! 调度队列维护、PID 管理、内核栈管理以及处理器当前任务状态管理。
//!
//! ## 模块组织
//!
//! - [`context`]   - 任务上下文 `TaskContext` 的保存与恢复
//! - [`manager`]   - 就绪队列管理与基本调度（FIFO）
//! - [`pid`]       - 进程 ID 分配与回收、内核栈管理
//! - [`processor`] - 当前处理器状态、当前任务获取、调度入口
//! - [`switch`]    - 低层上下文切换实现（汇编封装）
//! - [`task`]      - 任务控制块 `TaskControlBlock` 及其内部结构
//!
//! ## 公开接口（re-exports）
//!
//! - 类型：[`TaskContext`]
//! - 函数：[`add_task`], [`run_tasks`], [`schedule`], [`current_task`],
//!   [`current_trap_cx`], [`current_user_token`], [`take_current_task`],
//!   [`add_initproc`], [`suspend_current_and_run_next`], [`exit_current_and_run_next`]
//! - 常量：[`IDLE_PID`], [`INITPROC`]
//!
//! ## 调度模型
//!
//! - 调度策略：基于就绪队列的 FIFO 调度
//! - 切换路径：`run_tasks()` 选择下一个任务 → `__switch` 切到任务 →
//!   任务因时间片到期/主动让出/阻塞 → `schedule()` 切回调度器
//!
//! ## 信号（Signals）
//!
//! - 进程包含 `signals`/`signal_mask`/`signal_actions` 三部分状态：
//!   - `signals`：待处理信号集合
//!   - `signal_mask`：屏蔽集合（被屏蔽的信号不触发处理）
//!   - `signal_actions`：用户自定义处理动作表
//! - 处理流程要点：
//!   1. 进入内核后在合适时机检查 `signals` 与 `signal_mask`
//!   2. 对于致命信号，转换为退出码（如 SIGSEGV=-11 等）
//!   3. 对于可捕捉信号，按 `signal_actions` 进入用户处理程序，返回后 `sigreturn`
//! - 相关对外接口：[`check_signals_error_of_current`], [`current_add_signal`]
//!
//! ## 与系统调用的协作
//!
//! - 进程创建：[`sys_fork`] 深拷贝地址空间并返回父/子不同返回值
//! - 进程替换：[`sys_exec`] 用新 ELF 重建地址空间（成功不返回）
//! - 进程回收：[`sys_waitpid`] 回收子进程并写回退出码
//! - 让出 CPU：[`sys_yield`] 通过 [`suspend_current_and_run_next`]
//! - 退出：[`sys_exit`] 通过 [`exit_current_and_run_next`]
//!
//! ## 初始化与启动
//!
//! - 初始进程：[`INITPROC`]（从内置应用镜像加载 `initproc`）
//! - 空闲进程：PID 为 [`IDLE_PID`]（值为 0）的特殊进程，负责系统空闲时的处理
//! - 启动流程：调用 [`add_initproc()`] 将初始进程加入就绪队列，随后
//!   通过 [`run_tasks()`] 进入主调度循环
//!
//! ## 任务生命周期管理
//!
//! - 任务挂起：[`suspend_current_and_run_next()`] 将当前任务状态置为 `Ready` 并重新入队
//! - 任务退出：[`exit_current_and_run_next()`] 处理任务结束、孤儿进程托管和资源回收
//! - 空闲进程退出：当 PID 为 [`IDLE_PID`] 的进程退出时，根据退出码决定系统关机行为
//!
//! ## 使用示例
//!
//! ```rust
//! // 启动阶段：注册初始进程并进入调度循环
//! task::add_initproc();
//! // run_tasks() 在本工程由处理器模块统一驱动
//! ```
//!
use crate::fs::{OpenFlags, open_file};
use crate::{println, sbi::shutdown};
use alloc::sync::Arc;
use lazy_static::*;
use task::{TaskControlBlock, TaskStatus};

mod context;
mod manager;
mod pid;
mod processor;
mod signal;
mod switch;
#[allow(clippy::module_inception)]
mod task;

pub use context::TaskContext;
pub use manager::{add_task, pid2task, remove_from_pid2task};
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, take_current_task,
};
pub use signal::{MAX_SIG, SignalAction, SignalActions, SignalFlags};

lazy_static! {
    /// 初始进程（initproc）
    ///
    /// 调用open_file函数，打开initproc文件，并读取文件内容，创建TaskControlBlock
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new({
        let inode = open_file("initproc", OpenFlags::RDONLY).unwrap();
        let data = inode.read_all();
        TaskControlBlock::new(data.as_slice())
    });
}

/// 将初始进程加入就绪队列
///
/// 在系统启动阶段调用，把 [`INITPROC`] 推入调度器的就绪队列，等待
/// 调度器选择并运行。
pub fn add_initproc() {
    add_task(INITPROC.clone());
}

/// 让出当前任务并切换到下一个就绪任务
///
/// 将当前任务状态从 `Running` 置为 `Ready`，重新放回就绪队列，然后通过
/// [`schedule()`] 切换回调度器上下文，由调度器选择下一个任务运行。
///
/// ## 行为
/// - 保存当前任务上下文
/// - 更新任务状态为 `Ready`
/// - 重新入队就绪队列
/// - 触发上下文切换回调度器
pub fn suspend_current_and_run_next() {
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    add_task(task);
    schedule(task_cx_ptr);
}

/// 空闲进程的 PID
///
/// 值为 0 的特殊 PID，用于标识系统中的空闲进程。当空闲进程退出时，
/// 系统会根据其退出码决定是否关机：非零退出码触发错误关机，零退出码
/// 触发正常关机。
pub const IDLE_PID: usize = 0;

/// 结束当前任务并切换到下一个任务
///
/// 将当前任务标记为 `Zombie`，记录退出码，并进行"孤儿进程"托管：
/// 将其所有子进程的父指针重定向到 [`INITPROC`]。随后清空子进程列表、
/// 释放任务私有地址空间的区域元数据（不主动取消映射），最后切换回
/// 调度器，由调度器继续运行其他任务。
///
/// ## 特殊处理
/// - 如果当前任务 PID 为 [`IDLE_PID`]，则根据退出码决定系统关机行为：
///   - 非零退出码：触发错误关机
///   - 零退出码：触发正常关机
///
/// ## Arguments
/// * `exit_code` - 任务退出码
///
/// ## 备注
/// - 子进程在被重新托管后，退出回收将由 `initproc` 负责
/// - 地址空间的底层页帧由 RAII 管理，任务生命周期结束时被回收
pub fn exit_current_and_run_next(exit_code: i32) {
    let task = take_current_task().unwrap();

    let pid = task.getpid();
    if pid == IDLE_PID {
        println!(
            "[kernel] Idle process exit with exit_code {} ...",
            exit_code
        );
        if exit_code != 0 {
            shutdown(true)
        } else {
            shutdown(false)
        }
    }

    remove_from_pid2task(task.getpid());

    let mut inner = task.inner_exclusive_access();
    inner.task_status = TaskStatus::Zombie;
    inner.exit_code = exit_code;
    {
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    inner.children.clear();
    inner.memory_set.recycle_data_pages();

    if let Some(parent) = inner.parent.as_ref() {
        if let Some(parent_task) = parent.upgrade() {
            let mut parent_inner = parent_task.inner_exclusive_access();
            parent_inner.signals |= SignalFlags::SIGCHLD;
        }
    }

    drop(inner);
    drop(task);
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}

/// 检查当前任务的致命信号并返回标准退出码与原因
///
/// - 当 `signals` 集合包含致命/错误类信号（如 SIGSEGV、SIGILL 等）时，
///   返回对应的 `(exit_code, reason)`；否则返回 `None`。
/// - 该函数仅做快速判定，不会修改任务状态或触发调度。
pub fn check_signals_error_of_current() -> Option<(i32, &'static str)> {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    if !task_inner.signals.is_empty() {
        task_inner.signals.check_error()
    } else {
        None
    }
}

/// 向当前任务投递一个信号
///
/// - 将 `signal` 置入当前任务的 `signals` 集合，后续由调度路径调用
///   [`handle_signals`] 进行处理。
pub fn current_add_signal(signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    task_inner.signals |= signal;
}

/// 处理内核级信号的默认动作
///
/// - 支持内建处理：SIGSTOP（冻结）、SIGCONT（解冻）、其他视为 `killed=true`
/// - 仅修改内核维护的任务状态，不切换地址空间
fn call_kernel_signal_handler(signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    match signal {
        SignalFlags::SIGSTOP => {
            task_inner.frozen = true;
            task_inner.signals ^= SignalFlags::SIGSTOP;
        }
        SignalFlags::SIGCONT => {
            if task_inner.signals.contains(SignalFlags::SIGCONT) {
                task_inner.signals ^= SignalFlags::SIGCONT;
                task_inner.frozen = false;
            }
        }
        _ => {
            task_inner.killed = true;
        }
    }
}

/// 进入用户态信号处理程序
///
/// - 备份 Trap 上下文，设置 `sepc=handler`，`a0=sig`
/// - 标记 `handling_sig=sig`，并从 `signals` 中清除此信号位
fn call_user_signal_handler(sig: usize, signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();

    let handler = task_inner.signal_actions.table[sig].handler;
    if handler != 0 {
        task_inner.handling_sig = sig as isize;
        task_inner.signals ^= signal;

        let trap_ctx = task_inner.trap_cx();
        task_inner.trap_ctx_backup = Some(*trap_ctx);

        trap_ctx.sepc = handler;

        trap_ctx.x[10] = sig;
    } else {
        println!("[K] task/call_user_signal_handler: default action: ignore it or kill process");
    }
}

/// 扫描并处理一个可处理的待决信号
///
/// - 遍历 `0..=MAX_SIG`，考虑 `signal_mask` 与当前处理中的掩码规则
/// - 命中后调用 `call_kernel_signal_handler` 或 `call_user_signal_handler`
/// - 只处理至多一个信号，返回后由上层循环决定是否继续
fn check_pending_signals() {
    for sig in 0..(MAX_SIG + 1) {
        let task = current_task().unwrap();
        let task_inner = task.inner_exclusive_access();
        let signal = SignalFlags::from_bits(1 << sig).unwrap();
        if task_inner.signals.contains(signal) && (!task_inner.signal_mask.contains(signal)) {
            let mut masked = true;
            let handling_sig = task_inner.handling_sig;
            if handling_sig == -1 {
                masked = false;
            } else {
                let handling_sig = handling_sig as usize;
                if !task_inner.signal_actions.table[handling_sig]
                    .mask
                    .contains(signal)
                {
                    masked = false;
                }
            }
            if !masked {
                drop(task_inner);
                drop(task);
                if signal == SignalFlags::SIGKILL
                    || signal == SignalFlags::SIGSTOP
                    || signal == SignalFlags::SIGCONT
                    || signal == SignalFlags::SIGDEF
                {
                    call_kernel_signal_handler(signal);
                } else {
                    call_user_signal_handler(sig, signal);
                    return;
                }
            }
        }
    }
}

/// 处理当前任务的待决信号直至状态可继续执行
///
/// - 循环处理待决信号；若被冻结（SIGSTOP）则持续让出 CPU，直至 SIGCONT 或被 kill
/// - 若 `killed=true` 则结束循环，交由上层采取后续动作（如退出）
pub fn handle_signals() {
    loop {
        check_pending_signals();
        let (frozen, killed) = {
            let task = current_task().unwrap();
            let task_inner = task.inner_exclusive_access();
            (task_inner.frozen, task_inner.killed)
        };
        if !frozen || killed {
            break;
        }
        suspend_current_and_run_next();
    }
}
