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

use crate::loader::{get_app_data, get_num_app};
use crate::println;
use crate::sbi::shutdown;
use crate::sync::UPSafeCell;
use crate::task::switch::__switch;
use crate::trap::TrapContext;
use alloc::vec::Vec;
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
    tasks: Vec<TaskControlBlock>,

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
    /// 待补充
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        println!("init TaskManager, num_app: {}", num_app);
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
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
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let task0 = &mut inner.tasks[0];
        task0.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &task0.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
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
            println!("All applications completed!");
            shutdown(false);
        }
    }

    /// 获取当前任务的用户地址空间标识符
    ///
    /// 返回当前正在运行任务的用户地址空间页表标识符，
    /// 用于陷阱返回时切换到用户地址空间。
    ///
    /// ## Returns
    ///
    /// 当前任务的用户地址空间页表标识符（`satp` 寄存器值）
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].get_user_token()
    }

    /// 获取当前任务的陷阱上下文
    ///
    /// 返回当前正在运行任务的陷阱上下文的可变引用，
    /// 用于系统调用处理和异常处理。
    ///
    /// ## Returns
    ///
    /// 当前任务陷阱上下文的可变引用
    fn get_current_trap_cx(&self) -> &mut TrapContext {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].get_trap_cx()
    }

    /// 修改当前任务的程序断点
    ///
    /// 调整当前正在运行任务的堆大小，实现 `sbrk` 系统调用的功能。
    /// 这是任务管理器级别的堆管理接口。
    ///
    /// ## Arguments
    ///
    /// * `size` - 堆大小的变化量（字节）
    ///   - 正数：扩展堆空间
    ///   - 负数：收缩堆空间
    ///   - 零：查询当前断点位置
    ///
    /// ## Returns
    ///
    /// - `Some(old_brk)` - 成功时返回调整前的程序断点地址
    /// - `None` - 失败时返回 None
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].change_program_brk(size)
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
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// 将当前任务标记为挂起状态
///
/// 将当前运行任务的状态改为就绪状态，但不立即执行任务切换。
/// 通常与 [`run_next_task`] 结合使用实现完整的任务调度。
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// 将当前任务标记为已退出状态
///
/// 标记当前任务为已完成状态，该任务将不再被调度执行。
/// 通常在任务正常结束或因错误终止时调用。
fn mark_current_exited() {
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

/// 获取当前任务的用户地址空间标识符
///
/// 返回当前正在运行任务的用户地址空间页表标识符的公共接口。
/// 主要用于陷阱处理中的地址空间切换。
///
/// ## Returns
///
/// 当前任务的用户地址空间页表标识符（`satp` 寄存器值）
///
/// ## 使用场景
///
/// - **陷阱返回**: 在 `trap_return` 中切换回用户地址空间
/// - **地址转换**: 需要访问用户地址空间时的页表切换
/// - **调试工具**: 获取当前任务的地址空间信息
///
/// ## Examples
///
/// ```rust
/// // 在陷阱返回中使用
/// let user_satp = current_user_token();
/// unsafe {
///     satp::write(user_satp);
///     asm!("sfence.vma");
/// }
/// ```
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// 获取当前任务的陷阱上下文
///
/// 返回当前正在运行任务的陷阱上下文可变引用的公共接口。
/// 主要用于系统调用处理和异常处理中访问用户寄存器状态。
///
/// ## Returns
///
/// 当前任务陷阱上下文的可变引用，生命周期为 `'static`
///
/// ## 使用场景
///
/// - **系统调用处理**: 读取系统调用参数，设置返回值
/// - **异常处理**: 访问触发异常时的寄存器状态
/// - **信号处理**: 修改用户程序的执行上下文
/// - **调试工具**: 检查和修改任务状态
///
/// ## Safety
///
/// 返回 `'static` 生命周期的引用，调用者需要确保在任务切换前
/// 完成对陷阱上下文的所有访问。
///
/// ## Examples
///
/// ```rust
/// // 在系统调用处理中使用
/// let cx = current_trap_cx();
/// let syscall_id = cx.x[17];        // 读取系统调用号
/// let arg0 = cx.x[10];              // 读取第一个参数
/// cx.x[10] = result as usize;       // 设置返回值
/// ```
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// 修改当前任务的程序断点
///
/// 调整当前任务的堆大小的公共接口，实现 `sbrk` 系统调用。
/// 这是用户程序动态内存管理的核心接口。
///
/// ## Arguments
///
/// * `size` - 堆大小的变化量（字节）
///   - **正数**: 扩展堆空间，分配更多内存
///   - **负数**: 收缩堆空间，释放内存
///   - **零**: 查询当前程序断点位置
///
/// ## Returns
///
/// - `Some(old_brk)` - 成功时返回调整前的程序断点地址
/// - `None` - 失败时返回 None，可能的原因：
///   - 内存不足，无法分配新页面
///   - 试图收缩到堆底部以下
///   - 地址空间操作失败
///
/// ## 实现原理
///
/// 1. 委托给任务管理器的相应方法
/// 2. 任务管理器找到当前任务
/// 3. 调用任务控制块的堆管理方法
/// 4. 底层通过内存集合进行实际的页面分配/释放
///
/// ## 使用场景
///
/// - **`sbrk` 系统调用**: 用户程序动态调整堆大小
/// - **内存分配器**: `malloc`/`free` 的底层实现
/// - **垃圾收集器**: 动态调整堆空间
///
/// ## Examples
///
/// ```rust
/// // 扩展堆空间
/// if let Some(old_brk) = change_program_brk(4096) {
///     println!("Heap expanded from {:#x}", old_brk);
/// } else {
///     println!("Failed to expand heap");
/// }
///
/// // 查询当前断点
/// if let Some(current_brk) = change_program_brk(0) {
///     println!("Current program break: {:#x}", current_brk);
/// }
/// ```
#[allow(unused)]
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}
