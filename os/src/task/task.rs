//! # 任务控制块和任务状态
//!
//! 定义任务的基本数据结构和状态枚举，用于任务管理和调度。

use super::context::TaskContext;
use crate::{
    config::{TRAP_CONTEXT, kernel_stack_position},
    mm::{KERNEL_SPACE, MapPermission, MemorySet, PhysPageNum, VirtAddr},
    trap::{TrapContext, trap_handler},
};

/// 任务控制块 (Task Control Block, TCB)
///
/// 存储单个任务的所有必要信息，包括任务状态和执行上下文。
/// 每个应用程序对应一个任务控制块。
///
/// ## 设计原则
///
/// - **最小化设计**: 只包含任务调度所需的核心信息
/// - **状态分离**: 将任务状态与上下文分开存储，便于管理
/// - **复制语义**: 实现 `Copy` trait，支持高效的值拷贝
///
/// ## 扩展性
///
/// 未来版本可能会添加更多字段：
/// - 任务优先级
/// - 执行时间统计
/// - 内存使用信息
/// - 进程 ID 和父子关系
pub struct TaskControlBlock {
    /// 任务当前状态
    ///
    /// 标识任务在其生命周期中的当前阶段，用于调度决策。
    pub task_status: TaskStatus,

    /// 任务上下文
    ///
    /// 保存任务的 CPU 寄存器状态，用于任务切换时的状态保存和恢复。
    pub task_cx: TaskContext,

    /// 任务的内存地址空间
    ///
    /// 包含该任务的完整虚拟地址空间，包括代码段、数据段、堆、栈等。
    /// 每个任务都有独立的地址空间，实现进程间的内存隔离。
    pub memory_set: MemorySet,

    /// 陷阱上下文的物理页号
    ///
    /// 指向存储该任务陷阱上下文的物理页面。陷阱上下文存储在用户地址空间
    /// 的固定虚拟地址 `TRAP_CONTEXT`，此字段记录其对应的物理页号，
    /// 便于内核直接访问和修改。
    pub trap_cx_ppn: PhysPageNum,

    /// 基础大小（用户栈顶地址）
    ///
    /// 记录用户程序的初始栈顶地址，也作为堆空间的起始参考点。
    /// 在任务创建时设置，通常不会改变。
    #[allow(unused)]
    pub base_size: usize,

    /// 堆底部地址
    ///
    /// 用户程序堆空间的起始地址，通常等于 `base_size`。
    /// 堆从此地址开始向高地址方向增长，是 `sbrk` 系统调用的参考基准。
    pub heap_bottom: usize,

    /// 程序断点（当前堆顶）
    ///
    /// 当前用户程序堆空间的结束地址，即堆顶位置。
    /// 通过 `sbrk` 系统调用可以调整此值来扩展或收缩堆空间。
    /// 范围：`[heap_bottom, program_brk)` 为已分配的堆空间。
    pub program_brk: usize,
}

/// 任务状态枚举
///
/// 定义任务在其生命周期中可能处于的各种状态。
/// 任务状态转换遵循特定的规则，确保系统调度的正确性。
///
/// ## 状态转换图
///
/// ```text
/// ┌─────────┐    load     ┌───────┐    schedule   ┌─────────┐
/// │ Uninit  │ ──────────> │ Ready │ ────────────> │ Running │
/// └─────────┘             └───────┘               └─────────┘
///                            ^                         │
///                            │          yield/         │ exit/
///                            │         timeout         │ error
///                            └─────────────────────────┘
///                                                      │
///                                                      v
///                                              ┌─────────────┐
///                                              │   Exited    │
///                                              └─────────────┘
/// ```
///
/// ## 状态说明
///
/// - **Uninit**: 任务尚未初始化，仅在系统启动时短暂存在
/// - **Ready**: 任务已准备就绪，等待被调度执行
/// - **Running**: 任务正在 CPU 上执行
/// - **Exited**: 任务已完成执行，不会再被调度
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// 未初始化状态
    ///
    /// 任务刚创建时的初始状态，表示任务控制块已分配但尚未完成初始化。
    /// 这个状态在系统启动过程中短暂存在，很快会转换为 Ready 状态。
    Uninit,

    /// 就绪状态
    ///
    /// 任务已准备好执行，正在等待获得 CPU 时间片。
    /// 处于此状态的任务会被调度器考虑进行调度。
    Ready,

    /// 运行状态
    ///
    /// 任务正在 CPU 上执行。在单 CPU 系统中，同时只能有一个任务处于此状态。
    /// 任务可能因为时间片用完、主动让出或异常而离开此状态。
    Running,

    /// 已退出状态
    ///
    /// 任务已完成执行并退出，不会再被调度。
    /// 任务可能因为正常结束、调用 exit() 或发生致命错误而进入此状态。
    Exited,
}

impl TaskControlBlock {
    /// 创建新的任务控制块
    ///
    /// 从 ELF 文件数据创建一个完整的任务控制块，包括解析 ELF 文件、
    /// 构建用户地址空间、分配内核栈、初始化陷阱上下文等所有必要步骤。
    ///
    /// ## Arguments
    ///
    /// * `elf_data` - 应用程序的 ELF 文件二进制数据
    /// * `app_id` - 应用程序标识符，用于分配独立的内核栈
    ///
    /// ## Returns
    ///
    /// 完全初始化的任务控制块，可以被调度执行
    ///
    /// ## 初始化流程
    ///
    /// 1. **解析 ELF**: 调用 `MemorySet::from_elf()` 构建用户地址空间
    /// 2. **获取陷阱上下文**: 通过虚拟地址转换获取陷阱上下文的物理页号
    /// 3. **分配内核栈**: 在内核地址空间中为该任务分配独立的内核栈
    /// 4. **创建任务上下文**: 设置任务上下文指向 `trap_return`
    /// 5. **初始化陷阱上下文**: 设置用户程序入口、栈指针和内核环境
    /// 6. **设置堆管理**: 初始化堆底部和程序断点
    ///
    /// ## 内存布局
    ///
    /// ```text
    /// 用户地址空间:
    /// ┌─────────────────┐ ← TRAMPOLINE
    /// │   Trampoline    │
    /// ├─────────────────┤ ← TRAP_CONTEXT  
    /// │  Trap Context   │ ← trap_cx_ppn 指向这里
    /// ├─────────────────┤
    /// │   User Stack    │ ← user_sp
    /// ├─────────────────┤
    /// │   ELF Sections  │ ← entry_point
    /// └─────────────────┘
    ///
    /// 内核地址空间:
    /// ┌─────────────────┐ ← kernel_stack_top
    /// │  Kernel Stack   │ ← 每个任务独立的内核栈
    /// └─────────────────┘ ← kernel_stack_bottom
    /// ```
    ///
    /// ## 字段初始化
    ///
    /// - `task_status`: 设置为 `Ready` 状态
    /// - `task_cx`: 指向 `trap_return` 的任务上下文
    /// - `memory_set`: 从 ELF 构建的完整用户地址空间
    /// - `trap_cx_ppn`: 陷阱上下文的物理页号
    /// - `base_size`: 用户栈顶地址（堆的初始大小）
    /// - `heap_bottom`: 堆的起始地址
    /// - `program_brk`: 程序断点（当前堆顶）
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let app_data = get_app_data(0);
    /// let task = TaskControlBlock::new(app_data, 0);
    /// // 任务现在可以被调度执行
    /// ```
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;

        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
            heap_bottom: user_sp,
            program_brk: user_sp,
        };
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }

    /// 获取任务的陷阱上下文
    ///
    /// 通过陷阱上下文的物理页号获取该任务陷阱上下文的可变引用。
    /// 陷阱上下文存储在任务的用户地址空间中的固定虚拟地址。
    ///
    /// ## Returns
    ///
    /// 指向任务陷阱上下文的可变引用，生命周期为 `'static`
    ///
    /// ## 实现原理
    ///
    /// 1. **物理页号**: `trap_cx_ppn` 存储陷阱上下文所在的物理页号
    /// 2. **直接访问**: 通过物理页号直接获取物理内存的可变引用
    /// 3. **类型转换**: 将物理页面解释为 `TrapContext` 结构体
    ///
    /// ## 使用场景
    ///
    /// - **系统调用处理**: 读取和修改系统调用参数和返回值
    /// - **异常处理**: 访问触发异常时的寄存器状态
    /// - **任务初始化**: 设置新任务的初始陷阱上下文
    /// - **调试和诊断**: 检查任务的执行状态
    ///
    /// ## Safety
    ///
    /// 此函数返回 `'static` 生命周期的引用，调用者需要确保：
    /// - 不会在任务销毁后继续使用该引用
    /// - 同一时间只有一个任务在访问其陷阱上下文
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 获取当前任务的陷阱上下文
    /// let trap_cx = task.get_trap_cx();
    ///
    /// // 读取系统调用参数
    /// let syscall_id = trap_cx.x[17];
    /// let arg0 = trap_cx.x[10];
    ///
    /// // 设置系统调用返回值
    /// trap_cx.x[10] = result as usize;
    /// ```
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }

    /// 获取用户地址空间的页表标识符
    ///
    /// 返回该任务用户地址空间的页表标识符（`satp` 寄存器值），
    /// 用于地址空间切换和陷阱处理。
    ///
    /// ## Returns
    ///
    /// 用户地址空间的页表标识符，包含页表模式、ASID 和根页表物理页号
    ///
    /// ## 使用场景
    ///
    /// - **陷阱返回**: 在 `trap_return` 中切换回用户地址空间
    /// - **任务切换**: 保存和恢复任务的地址空间标识符
    /// - **调试工具**: 获取任务的地址空间信息
    /// - **内存管理**: 识别不同任务的地址空间
    ///
    /// ## 实现细节
    ///
    /// 此函数是 `self.memory_set.token()` 的便捷包装，
    /// 提供了任务级别的接口来访问底层的内存管理功能。
    ///
    /// ## 与内核地址空间的区别
    ///
    /// - **用户 token**: 每个任务都有独立的用户地址空间标识符
    /// - **内核 token**: 所有任务共享同一个内核地址空间标识符
    /// - **切换时机**: 在陷阱处理和任务切换时需要在两者间切换
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 获取用户地址空间标识符
    /// let user_token = task.get_user_token();
    ///
    /// // 在陷阱返回时使用
    /// unsafe {
    ///     satp::write(user_token);
    ///     asm!("sfence.vma");
    /// }
    /// ```
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }

    /// 修改程序断点（堆大小调整）
    ///
    /// 实现 `sbrk` 系统调用的核心功能，通过调整程序断点来动态改变
    /// 用户程序的堆大小。支持堆的扩展和收缩操作。
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
    /// - `None` - 失败时返回 None（通常是内存不足或参数无效）
    ///
    /// ## 堆管理机制
    ///
    /// ```text
    /// 堆内存布局:
    /// ┌─────────────────┐ ← heap_bottom (固定)
    /// │                 │
    /// │   已分配堆空间   │
    /// │                 │
    /// ├─────────────────┤ ← program_brk (可变)
    /// │                 │
    /// │   未分配空间     │
    /// │                 │
    /// └─────────────────┘
    /// ```
    ///
    /// ## 操作流程
    ///
    /// 1. **计算新断点**: `new_brk = current_brk + size`
    /// 2. **边界检查**: 确保新断点不低于 `heap_bottom`
    /// 3. **内存操作**:
    ///    - 扩展：调用 `memory_set.append_to()` 分配新页面
    ///    - 收缩：调用 `memory_set.shrink_to()` 释放页面
    /// 4. **更新断点**: 成功后更新 `program_brk` 字段
    ///
    /// ## 错误情况
    ///
    /// - **堆收缩过度**: 新断点低于 `heap_bottom`
    /// - **内存不足**: 无法分配足够的物理页面
    /// - **地址空间冲突**: 与其他内存区域重叠
    ///
    /// ## 并发安全
    ///
    /// 此函数需要 `&mut self`，确保同一时间只有一个线程能修改
    /// 任务的堆状态，避免竞争条件。
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 扩展堆空间 4KB
    /// if let Some(old_brk) = task.change_program_brk(4096) {
    ///     println!("Heap extended from {:#x}", old_brk);
    /// }
    ///
    /// // 收缩堆空间 2KB  
    /// if let Some(old_brk) = task.change_program_brk(-2048) {
    ///     println!("Heap shrunk from {:#x}", old_brk);
    /// }
    ///
    /// // 查询当前断点
    /// if let Some(current_brk) = task.change_program_brk(0) {
    ///     println!("Current program break: {:#x}", current_brk);
    /// }
    /// ```
    #[allow(unused)]
    pub fn change_program_brk(&mut self, size: i32) -> Option<usize> {
        let old_break = self.program_brk;
        let new_brk = self.program_brk as isize + size as isize;
        if new_brk < self.heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            self.memory_set
                .shrink_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        } else {
            self.memory_set
                .append_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            self.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }
}
