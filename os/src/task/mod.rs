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
//! - 类型：[`TaskContext`], [`Processor`]
//! - 函数：[`add_task`], [`run_tasks`], [`schedule`], [`current_task`],
//!   [`current_trap_cx`], [`current_user_token`], [`take_current_task`]
//! - PID/栈：[`PidAllocator`], [`PidHandle`], [`KernelStack`], [`pid_alloc`]
//!
//! ## 调度模型
//!
//! - 调度策略：基于就绪队列的 FIFO 调度
//! - 切换路径：`run_tasks()` 选择下一个任务 → `__switch` 切到任务 →
//!   任务因时间片到期/主动让出/阻塞 → `schedule()` 切回调度器
//!
//! ## 初始化与启动
//!
//! - 初始进程：[`INITPROC`]（从内置应用镜像加载 `initproc`）
//! - 启动流程：调用 [`add_initproc()`] 将初始进程加入就绪队列，随后
//!   通过 [`run_tasks()`] 进入主调度循环
//!
//! ## 使用示例
//!
//! ```rust
//! // 启动阶段：注册初始进程并进入调度循环
//! task::add_initproc();
//! // run_tasks() 在本工程由处理器模块统一驱动
//! ```
//!
use crate::loader::get_app_data_by_name;
use alloc::sync::Arc;
use lazy_static::*;
use task::{TaskControlBlock, TaskStatus};

mod context;
mod manager;
mod pid;
mod processor;
mod switch;
#[allow(clippy::module_inception)]
mod task;

pub use context::TaskContext;
pub use manager::add_task;
pub use pid::{KernelStack, PidAllocator, PidHandle, pid_alloc};
pub use processor::{
    Processor, current_task, current_trap_cx, current_user_token, run_tasks, schedule,
    take_current_task,
};

lazy_static! {
    /// 初始进程（initproc）
    ///
    /// 从内核内置应用仓库中加载名为 `initproc` 的程序，作为系统中的第一个
    /// 用户进程。该进程通常负责拉起其他用户程序或提供最小的用户空间环境。
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new(TaskControlBlock::new(
        get_app_data_by_name("initproc").unwrap()
    ));
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

/// 结束当前任务并切换到下一个任务
///
/// 将当前任务标记为 `Zombie`，记录退出码，并进行“孤儿进程”托管：
/// 将其所有子进程的父指针重定向到 [`INITPROC`]。随后清空子进程列表、
/// 释放任务私有地址空间的区域元数据（不主动取消映射），最后切换回
/// 调度器，由调度器继续运行其他任务。
///
/// ## Arguments
/// * `exit_code` - 任务退出码
///
/// ## 备注
/// - 子进程在被重新托管后，退出回收将由 `initproc` 负责
/// - 地址空间的底层页帧由 RAII 管理，任务生命周期结束时被回收
pub fn exit_current_and_run_next(exit_code: i32) {
    let task = take_current_task().unwrap();
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
    drop(inner);
    drop(task);
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}
