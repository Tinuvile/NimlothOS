//! # 任务管理模块
//!
//! 实现协作式多任务调度系统，支持多个用户应用程序的并发执行。
//! 采用时间片轮转调度算法，通过上下文切换在不同任务间切换。
//!
//! ## 核心组件
//!
//! - [`TaskManager`] - 任务管理器，负责所有任务的生命周期管理
//! - [`TaskControlBlock`] - 任务控制块，存储单个任务的状态信息
//! - [`TaskContext`] - 任务上下文，保存 CPU 寄存器状态
//! - [`TaskStatus`] - 任务状态枚举
//!
//! ## 调度策略
//!
//! 使用简单的时间片轮转 (Round Robin) 调度：
//! - 每个任务获得固定的时间片
//! - 时间片用完或主动让出时切换到下一个就绪任务
//! - 任务按 ID 顺序循环调度
//!
//! ## 任务状态转换
//!
//! ```text
//! Uninit -> Ready -> Running -> Exited
//!             ^         |
//!             |         v  
//!             +--- Ready (yield)
//! ```

use crate::loader::{get_num_app, init_app_cx};
use crate::task::switch::__switch;
use crate::{config::MAX_APP_NUM, sync::UPSafeCell};
use lazy_static::*;
use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

mod context;
mod switch;

#[allow(clippy::module_inception)]
mod task;

/// 任务管理器
///
/// 负责管理系统中所有任务的生命周期，包括任务创建、调度和销毁。
/// 使用 [`UPSafeCell`] 提供线程安全的内部可变性。
///
/// ## 设计特点
///
/// - **静态任务数组**: 预先分配固定数量的任务槽位
/// - **轮转调度**: 使用简单的轮转算法选择下一个任务
/// - **状态管理**: 跟踪每个任务的执行状态
/// - **上下文保存**: 保存和恢复任务的 CPU 状态
pub struct TaskManager {
    /// 系统中的应用程序数量
    num_app: usize,
    /// 任务管理器的内部状态，使用 UPSafeCell 保证线程安全
    inner: UPSafeCell<TaskManagerInner>,
}

/// 任务管理器内部状态
///
/// 包含实际的任务数组和当前运行任务的索引。
/// 该结构体被 [`UPSafeCell`] 包装以提供安全的并发访问。
pub struct TaskManagerInner {
    /// 所有任务的控制块数组
    ///
    /// 数组大小固定为 [`MAX_APP_NUM`]，每个槽位对应一个应用程序。
    tasks: [TaskControlBlock; MAX_APP_NUM],

    /// 当前正在运行的任务 ID
    ///
    /// 用作 `tasks` 数组的索引，指向当前获得 CPU 时间片的任务。
    current_task: usize,
}

lazy_static! {
    /// 全局任务管理器实例
    ///
    /// 系统启动时创建的单例任务管理器，负责管理所有用户应用程序。
    /// 使用 `lazy_static!` 确保在首次访问时才进行初始化。
    ///
    /// ## 初始化过程
    ///
    /// 1. 获取应用程序数量
    /// 2. 为每个应用程序创建任务控制块
    /// 3. 设置任务上下文指向陷阱恢复入口
    /// 4. 将所有任务标记为就绪状态
    /// 5. 设置第一个任务为当前任务
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let mut tasks = [TaskControlBlock {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::Uninit,
        }; MAX_APP_NUM];
        for (i, task) in tasks.iter_mut().enumerate() {
            task.task_cx = TaskContext::goto_restore(init_app_cx(i));
            task.task_status = TaskStatus::Ready;
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// 启动第一个任务
    ///
    /// 系统初始化完成后调用此函数开始任务调度。
    /// 该函数会将第一个任务（ID 为 0）设置为运行状态，
    /// 然后执行上下文切换跳转到用户任务。
    ///
    /// ## 执行流程
    ///
    /// 1. 获取任务管理器内部状态的独占访问
    /// 2. 将第一个任务状态设置为 `Running`
    /// 3. 获取任务上下文指针
    /// 4. 释放内部状态锁
    /// 5. 执行上下文切换到第一个任务
    ///
    /// ## Panics
    ///
    /// 如果上下文切换意外返回，会触发 panic（正常情况下不应该发生）
    fn run_first_task(&self) {
        let mut inner = self.inner.exclusive_access();
        let task0 = &mut inner.tasks[0];
        task0.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &task0.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        unsafe {
            __switch(&mut _unused as *mut TaskContext, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// 将当前任务标记为挂起状态
    ///
    /// 将当前正在运行的任务状态从 `Running` 改为 `Ready`，
    /// 表示该任务可以被重新调度执行。通常在任务主动让出 CPU 时调用。
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;
        inner.tasks[current_task].task_status = TaskStatus::Ready;
    }

    /// 将当前任务标记为已退出状态
    ///
    /// 将当前正在运行的任务状态设置为 `Exited`，
    /// 表示该任务已经完成执行，不会再被调度。
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    /// 查找下一个可运行的任务
    ///
    /// 使用轮转调度算法在当前任务之后查找下一个状态为 `Ready` 的任务。
    /// 搜索顺序是从当前任务的下一个开始，循环遍历所有任务。
    ///
    /// ## Returns
    ///
    /// - `Some(task_id)` - 找到的下一个就绪任务的 ID
    /// - `None` - 没有找到就绪任务
    ///
    /// ## 调度算法
    ///
    /// 轮转调度 (Round Robin)：
    /// ```text
    /// 当前任务ID = 2, 任务总数 = 4
    /// 搜索顺序: 3 -> 0 -> 1 -> (回到2)
    /// ```
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;

        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// 切换到下一个任务
    ///
    /// 查找下一个就绪任务并执行任务切换。如果找到了就绪任务，
    /// 会保存当前任务的上下文，恢复目标任务的上下文，然后跳转执行。
    ///
    /// ## 执行流程
    ///
    /// 1. 查找下一个就绪任务
    /// 2. 如果找到：
    ///    - 将目标任务状态设置为 `Running`
    ///    - 更新当前任务索引
    ///    - 执行上下文切换
    /// 3. 如果没找到：触发 panic（所有任务都已完成）
    ///
    /// ## Panics
    ///
    /// 当没有任何就绪任务时会触发 panic，表示所有应用程序都已完成执行
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            panic!("All applications completed!");
        }
    }
}

/// 启动第一个任务
///
/// 系统启动时调用的公共接口，开始执行第一个用户任务。
/// 这是任务调度系统的入口点，调用后内核将转移控制权给用户任务。
///
/// ## 调用时机
///
/// 应在以下系统初始化步骤完成后调用：
/// - 中断系统初始化
/// - 应用程序加载完成
/// - 时钟中断启用
///
/// ## Note
///
/// 此函数调用后不会返回，因为控制权完全转移给用户任务。
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// 切换到下一个就绪任务
///
/// 任务调度的核心接口，用于实现抢占式和协作式任务切换。
/// 该函数会查找下一个就绪任务并执行上下文切换。
///
/// ## 使用场景
///
/// - 时钟中断触发的抢占式调度
/// - 任务主动让出 CPU 的协作式调度
/// - 当前任务退出后的任务切换
pub fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// 将当前任务标记为挂起状态
///
/// 将当前运行任务的状态改为就绪状态，但不立即执行任务切换。
/// 通常与 [`run_next_task`] 结合使用实现完整的任务调度。
pub fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// 将当前任务标记为已退出状态
///
/// 标记当前任务为已完成状态，该任务将不再被调度执行。
/// 通常在任务正常结束或因错误终止时调用。
pub fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// 挂起当前任务并切换到下一个任务
///
/// 协作式任务调度的主要接口。当任务主动让出 CPU 时调用，
/// 会将当前任务标记为就绪状态，然后切换到下一个就绪任务。
///
/// ## 使用场景
///
/// - 实现 `yield()` 系统调用
/// - 任务等待某些条件时主动让出 CPU
/// - 时钟中断处理中的抢占式调度
///
/// ## 执行流程
///
/// 1. 将当前任务状态设置为 `Ready`
/// 2. 查找下一个就绪任务
/// 3. 执行上下文切换
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// 终止当前任务并切换到下一个任务
///
/// 任务退出时的调度接口。将当前任务标记为已退出状态，
/// 然后切换到下一个就绪任务继续执行。
///
/// ## 使用场景
///
/// - 实现 `exit()` 系统调用
/// - 任务因异常终止时的清理
/// - 应用程序正常结束时的处理
///
/// ## 执行流程
///
/// 1. 将当前任务状态设置为 `Exited`
/// 2. 查找下一个就绪任务
/// 3. 执行上下文切换（如果有就绪任务）
/// 4. 如果没有就绪任务，系统结束运行
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}
