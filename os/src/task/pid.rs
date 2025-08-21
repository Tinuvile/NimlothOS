//! # 进程 ID 和内核栈管理模块
//!
//! 本模块提供了进程标识符 (PID) 的分配、回收以及内核栈的生命周期管理功能。
//! 是操作系统任务管理的核心基础设施之一。
//!
//! ## 核心功能
//!
//! ### 进程 ID 管理
//! - **PID 分配**: 为新创建的进程分配唯一的数字标识符
//! - **PID 回收**: 回收已终止进程的 PID，实现资源复用
//! - **重复分配检测**: 防止同一 PID 被重复回收，确保系统一致性
//! - **RAII 管理**: 通过 `PidHandle` 自动管理 PID 的生命周期
//!
//! ### 内核栈管理
//! - **栈空间分配**: 为每个进程分配独立的内核栈空间
//! - **虚拟内存映射**: 将内核栈映射到内核地址空间
//! - **自动清理**: 进程销毁时自动回收内核栈资源
//! - **栈顶操作**: 支持在内核栈顶部压入数据
//!
//! ## 设计原理
//!
//! ### PID 分配策略
//! 采用混合分配策略，优先复用已回收的 PID：
//! 1. 检查回收池中是否有可用的 PID
//! 2. 如果回收池为空，则分配新的递增 PID
//! 3. 通过 `PidHandle` 提供 RAII 风格的自动回收
//!
//! ### 内核栈布局
//! 每个进程的内核栈在虚拟地址空间中的布局：
//!
//! ```text
//! 高地址 TRAMPOLINE (0x3ffffff000)
//!         ↓
//!     ┌─────────────────────────────────┐
//!     │        Guard Page               │ ← 4KB 保护页
//!     ├─────────────────────────────────┤
//!     │     Process 0 Kernel Stack      │ ← 8KB 栈空间
//!     ├─────────────────────────────────┤
//!     │        Guard Page               │ ← 4KB 保护页
//!     ├─────────────────────────────────┤
//!     │     Process 1 Kernel Stack      │ ← 8KB 栈空间
//!     ├─────────────────────────────────┤
//!     │           ...                   │
//!         ↓
//! 低地址
//! ```
//!
//! ## 安全保证
//!
//! - **内存安全**: 所有内存操作都经过虚拟内存管理器验证
//! - **PID 唯一性**: 防止 PID 重复分配和重复回收
//! - **栈隔离**: 每个进程的内核栈完全独立，避免相互干扰
//! - **自动清理**: 通过 RAII 确保资源不会泄漏
//!
//! ## 使用示例
//!
//! ```rust
//! // 分配新的进程 ID
//! let pid_handle = pid_alloc();
//! println!("Allocated PID: {}", pid_handle.0);
//!
//! // 创建对应的内核栈
//! let kernel_stack = KernelStack::new(&pid_handle);
//!
//! // 在栈顶压入数据
//! let trap_context_ptr = kernel_stack.push_on_top(TrapContext::default());
//!
//! // 变量离开作用域时，PID 和内核栈会自动回收
//! ```

use crate::{
    config::{KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE},
    mm::{KERNEL_SPACE, MapPermission, VirtAddr},
    sync::UPSafeCell,
};
use alloc::vec::Vec;
use core::usize;
use lazy_static::*;

/// 进程 ID 句柄
///
/// 对进程标识符的封装，提供 RAII 风格的自动生命周期管理。当 `PidHandle`
/// 离开作用域时，会自动将持有的 PID 归还给分配器，实现资源的自动回收。
///
/// ## 设计特性
///
/// - **唯一所有权**: 每个 `PidHandle` 唯一拥有一个 PID
/// - **移动语义**: 支持所有权转移，避免意外的重复释放
/// - **自动回收**: 通过 `Drop` trait 实现自动资源管理
/// - **零开销抽象**: 编译时优化为原始 usize 访问
///
/// ## 内部表示
///
/// 内部存储一个 `usize` 类型的 PID 值，该值保证在系统范围内唯一。
/// PID 从 0 开始分配，依次递增，回收的 PID 会被优先重新分配。
///
/// ## Examples
///
/// ```rust
/// // 分配新的 PID
/// let pid_handle = pid_alloc();
/// let pid_value = pid_handle.0;
///
/// // 转移所有权
/// let task = Task::new(pid_handle);
///
/// // 当 task 被销毁时，PID 会自动回收
/// ```
pub struct PidHandle(pub usize);

/// 进程 ID 分配器
///
/// 管理系统中所有进程 ID 的分配和回收，采用高效的混合分配策略。
/// 优先复用已回收的 PID，在回收池为空时分配新的递增 PID。
///
/// ## 分配策略
///
/// ### 分配顺序
/// 1. **优先回收**: 首先检查回收池中是否有可用的 PID
/// 2. **递增分配**: 回收池为空时，分配新的连续 PID
/// 3. **唯一性保证**: 确保系统中不存在重复的活跃 PID
///
/// ### 数据结构
/// - `current`: 下一个要分配的新 PID 值（单调递增）
/// - `recycled`: 已回收的 PID 池，使用向量实现的栈结构
///
/// ## 算法复杂度
///
/// - **分配操作**: O(1) - 无论是从回收池还是新分配
/// - **回收操作**: O(n) - 需要检查重复回收（n 为回收池大小）
/// - **空间复杂度**: O(r) - r 为已回收但未重新分配的 PID 数量
///
/// ## 线程安全
///
/// 本结构体本身不提供线程安全保证，需要配合 `UPSafeCell` 等同步原语使用。
/// 全局实例 `PID_ALLOCATOR` 通过互斥访问确保线程安全。
pub struct PidAllocator {
    /// 下一个要分配的新 PID（从 0 开始递增）
    current: usize,
    /// 回收的 PID 池，后进先出的栈结构
    recycled: Vec<usize>,
}

impl PidAllocator {
    /// 创建新的 PID 分配器
    ///
    /// 初始化一个空的分配器，PID 从 0 开始分配，回收池为空。
    /// 这是系统启动时调用的构造函数。
    ///
    /// ## Returns
    ///
    /// 返回初始化完成的 `PidAllocator` 实例
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let allocator = PidAllocator::new();
    /// assert_eq!(allocator.current, 0);
    /// assert!(allocator.recycled.is_empty());
    /// ```
    pub fn new() -> Self {
        PidAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }

    /// 分配一个新的进程 ID
    ///
    /// 根据分配策略返回一个可用的 PID。优先从回收池中取出已释放的 PID，
    /// 如果回收池为空，则分配下一个连续的新 PID。
    ///
    /// ## 分配逻辑
    ///
    /// ```text
    /// 检查回收池
    ///     ↓
    /// 有可用PID? ───Yes──→ 返回回收的PID
    ///     ↓ No
    /// 分配新PID (current)
    ///     ↓
    /// current += 1
    ///     ↓
    /// 返回新分配的PID
    /// ```
    ///
    /// ## Returns
    ///
    /// 返回包装在 `PidHandle` 中的唯一 PID
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let mut allocator = PidAllocator::new();
    ///
    /// // 分配前几个PID
    /// let pid1 = allocator.alloc(); // PID = 0
    /// let pid2 = allocator.alloc(); // PID = 1
    /// let pid3 = allocator.alloc(); // PID = 2
    ///
    /// // 回收 PID 1
    /// allocator.dealloc(1);
    ///
    /// // 下次分配会复用回收的 PID
    /// let pid4 = allocator.alloc(); // PID = 1 (复用)
    /// ```
    pub fn alloc(&mut self) -> PidHandle {
        if let Some(pid) = self.recycled.pop() {
            PidHandle(pid)
        } else {
            self.current += 1;
            PidHandle(self.current - 1)
        }
    }

    /// 回收一个进程 ID
    ///
    /// 将已使用完毕的 PID 放回回收池中，供后续分配复用。包含完整的
    /// 安全检查以防止重复回收和无效回收。
    ///
    /// ## 安全检查
    ///
    /// 1. **有效性检查**: 确保 PID 小于当前分配的最大值
    /// 2. **重复检查**: 防止同一 PID 被多次回收
    /// 3. **系统一致性**: 维护分配器内部状态的正确性
    ///
    /// ## Arguments
    ///
    /// * `pid` - 要回收的进程 ID，必须是之前通过 `alloc()` 分配的有效 PID
    ///
    /// ## Panics
    ///
    /// * 如果 `pid >= current`，表示试图回收未分配的 PID
    /// * 如果 `pid` 已经在回收池中，表示重复回收
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let mut allocator = PidAllocator::new();
    ///
    /// // 分配 PID
    /// let handle1 = allocator.alloc(); // PID = 0
    /// let handle2 = allocator.alloc(); // PID = 1
    ///
    /// // 正确回收
    /// allocator.dealloc(0); // ✓ 正确
    ///
    /// // 以下操作会 panic
    /// // allocator.dealloc(0);  // ✗ 重复回收
    /// // allocator.dealloc(10); // ✗ 未分配的 PID
    /// ```
    ///
    /// ## 实现注意事项
    ///
    /// 回收检查的时间复杂度为 O(n)，其中 n 是回收池大小。在高频率的
    /// 进程创建/销毁场景下可能成为性能瓶颈，但对于系统安全性是必要的。
    pub fn dealloc(&mut self, pid: usize) {
        assert!(pid < self.current);
        assert!(
            !self.recycled.iter().any(|ppid| *ppid == pid),
            "pid {} has been deallocated!",
            pid
        );
        self.recycled.push(pid);
    }
}

lazy_static! {
    /// 全局进程 ID 分配器
    ///
    /// 系统唯一的 PID 分配器实例，提供线程安全的并发访问支持。
    /// 通过 `UPSafeCell` 封装确保在单处理器环境下的互斥访问。
    ///
    /// ## 设计特点
    ///
    /// - **全局唯一**: 整个系统只有一个 PID 分配器实例
    /// - **线程安全**: 通过互斥访问防止竞态条件
    /// - **延迟初始化**: 首次访问时才创建分配器实例
    /// - **自动管理**: 系统生命周期内保持状态一致性
    ///
    /// ## 使用模式
    ///
    /// 通过 `exclusive_access()` 方法获取对分配器的独占访问权，
    /// 确保同一时刻只有一个线程能修改分配器状态。
    ///
    /// ```rust
    /// // 获取独占访问权并执行操作
    /// let handle = PID_ALLOCATOR.exclusive_access().alloc();
    /// ```
    pub static ref PID_ALLOCATOR: UPSafeCell<PidAllocator> =
        unsafe { UPSafeCell::new(PidAllocator::new()) };
}

/// 分配新的进程 ID
///
/// 系统级的 PID 分配接口，为新创建的进程分配一个唯一的标识符。
/// 该函数是进程创建流程中的关键步骤，通常在任务创建时调用。
///
/// ## 实现细节
///
/// 1. **获取锁**: 通过 `exclusive_access()` 获取全局分配器的互斥访问权
/// 2. **执行分配**: 调用分配器的 `alloc()` 方法
/// 3. **返回句柄**: 返回包装了 PID 的 `PidHandle`
/// 4. **自动释放锁**: 函数结束时自动释放互斥访问权
///
/// ## Returns
///
/// 返回一个 `PidHandle`，包含新分配的唯一 PID。该句柄实现了 RAII，
/// 当其离开作用域时会自动将 PID 归还给分配器。
///
/// ## 线程安全
///
/// 此函数是线程安全的，可以在多个执行上下文中安全调用。
/// 内部的互斥访问机制确保不会产生竞态条件。
///
/// ## Examples
///
/// ```rust
/// use crate::task::pid_alloc;
///
/// // 为新进程分配 PID
/// let pid_handle = pid_alloc();
/// println!("分配了 PID: {}", pid_handle.0);
///
/// // 创建任务时使用
/// let task = TaskControlBlock::new(
///     app_data,
///     pid_handle, // 所有权转移给任务
/// );
///
/// // 当 task 被销毁时，PID 会自动回收
/// ```
///
/// ## 性能注意事项
///
/// - **锁竞争**: 在高并发情况下可能产生短暂的锁竞争
/// - **分配成本**: 单次调用的成本非常低（O(1) 或 O(n)，取决于回收池状态）
pub fn pid_alloc() -> PidHandle {
    PID_ALLOCATOR.exclusive_access().alloc()
}

/// `PidHandle` 的自动资源管理
///
/// 实现 `Drop` trait 为 `PidHandle` 提供 RAII 风格的自动资源管理。
/// 当 `PidHandle` 离开作用域时，会自动将持有的 PID 归还给全局分配器。
///
/// ## RAII 原理
///
/// RAII (Resource Acquisition Is Initialization) 确保：
/// - **获取即初始化**: PID 在 `PidHandle` 创建时分配
/// - **销毁即释放**: PID 在 `PidHandle` 销毁时自动回收
/// - **异常安全**: 即使发生 panic，资源也会被正确清理
/// - **无手动管理**: 开发者无需手动调用回收函数
///
/// ## 自动清理时机
///
/// `drop()` 方法会在以下情况下自动调用：
/// 1. **变量离开作用域**: 正常的语句块结束
/// 2. **显式销毁**: 调用 `drop(handle)` 显式销毁
/// 3. **重新赋值**: 变量被赋予新值时
/// 4. **程序终止**: 程序正常或异常终止时
/// 5. **panic 展开**: 即使发生 panic，也会正确清理
///
/// ## 实现细节
///
/// ```rust
/// fn drop(&mut self) {
///     // 1. 获取全局分配器的互斥访问权
///     // 2. 调用 dealloc() 回收 PID
///     // 3. 自动释放互斥访问权
///     PID_ALLOCATOR.exclusive_access().dealloc(self.0);
/// }
/// ```
///
/// ## 线程安全
///
/// `drop()` 方法是线程安全的，即使在多线程环境下也可以安全调用。
/// 内部使用的互斥访问机制确保操作原子性。
///
/// ## 性能特性
///
/// - **低开销**: `drop()` 操作非常轻量，主要开销是互斥锁获取
/// - **O(n) 复杂度**: 因为需要检查重复回收，时间复杂度与回收池大小相关
/// - **无内存分配**: 回收过程不需要额外的内存分配
impl Drop for PidHandle {
    fn drop(&mut self) {
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

/// 内核栈管理器
///
/// 为每个进程分配和管理独立的内核栈空间。内核栈用于存储系统调用、中断处理
/// 和任务切换时的临时数据。每个内核栈在虚拟地址空间中占用固定的位置。
///
/// ## 设计特性
///
/// ### 栈空间隔离
/// - **独立映射**: 每个进程拥有完全独立的内核栈空间
/// - **保护页**: 相邻栈之间有保护页防止栈溢出
/// - **固定大小**: 每个栈的大小为 `KERNEL_STACK_SIZE` (通常 8KB)
/// - **预定位置**: 栈位置由 PID 唯一确定，便于快速定位
///
/// ### 虚拟内存布局
///
/// ```text
/// TRAMPOLINE (高地址)
///     ↓
/// ┌─────────────────┐ ← PID 0 栈顶
/// │   Kernel Stack  │
/// │     (PID 0)     │ ← 8KB
/// └─────────────────┘ ← PID 0 栈底
/// ┌─────────────────┐ ← 保护页 (4KB)
/// └─────────────────┘
/// ┌─────────────────┐ ← PID 1 栈顶
/// │   Kernel Stack  │
/// │     (PID 1)     │ ← 8KB  
/// └─────────────────┘ ← PID 1 栈底
/// ┌─────────────────┐ ← 保护页 (4KB)
/// └─────────────────┘
///     ↓
/// (低地址方向)
/// ```
///
/// ## RAII 资源管理
///
/// - **自动分配**: 构造时自动分配并映射虚拟内存
/// - **自动回收**: 析构时自动解映射并释放内存
/// - **异常安全**: 即使发生异常也能正确清理资源
/// - **零泄漏**: 通过 Rust 的所有权系统保证无内存泄漏
///
/// ## 线程安全
///
/// 内核栈的分配和回收涉及全局内存管理器，通过互斥访问保证线程安全。
/// 但单个 `KernelStack` 实例本身不支持跨线程共享。
pub struct KernelStack {
    /// 关联的进程 ID，用于确定内核栈在虚拟地址空间中的位置
    pid: usize,
}

impl KernelStack {
    /// 为指定进程创建新的内核栈
    ///
    /// 根据进程 ID 计算内核栈的虚拟地址范围，并在内核地址空间中建立映射。
    /// 内核栈具有读写权限，用于存储系统调用和中断处理时的临时数据。
    ///
    /// ## Arguments
    ///
    /// * `pid_handle` - 进程 ID 句柄的引用，用于确定栈的虚拟地址位置
    ///
    /// ## Returns
    ///
    /// 返回初始化完成的 `KernelStack` 实例
    ///
    /// ## 实现过程
    ///
    /// 1. **地址计算**: 通过 `kernel_stack_position()` 计算栈的起始和结束地址
    /// 2. **内存映射**: 在内核地址空间中建立虚拟到物理的页面映射
    /// 3. **权限设置**: 设置页面的读写权限 (R|W)
    /// 4. **实例创建**: 创建并返回 `KernelStack` 实例
    ///
    /// ## 内存分配
    ///
    /// - **按需分配**: 物理页面在访问时才真正分配（延迟分配）
    /// - **连续虚拟地址**: 保证栈空间在虚拟地址上连续
    /// - **页面对齐**: 所有地址都按页边界对齐
    ///
    /// ## Panics
    ///
    /// 如果虚拟内存映射失败（如地址冲突或内存不足），函数会 panic
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 分配进程 ID
    /// let pid_handle = pid_alloc();
    ///
    /// // 创建对应的内核栈
    /// let kernel_stack = KernelStack::new(&pid_handle);
    ///
    /// // 现在可以使用内核栈进行系统调用处理
    /// let stack_top = kernel_stack.get_top();
    /// println!("Kernel stack top: 0x{:x}", stack_top);
    /// ```
    pub fn new(pid_handle: &PidHandle) -> Self {
        let pid = pid_handle.0;
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(pid);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        KernelStack { pid: pid_handle.0 }
    }

    /// 在内核栈顶压入数据
    ///
    /// 在内核栈的顶部压入一个值，并返回指向该值的可变指针。这是一个泛型方法，
    /// 可以压入任何实现了 `Sized` trait 的类型。主要用于在任务切换时保存上下文信息。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 要压入的数据类型，必须实现 `Sized` trait
    ///
    /// ## Arguments
    ///
    /// * `value` - 要压入栈顶的值，通过值传递（移动语义）
    ///
    /// ## Returns
    ///
    /// 返回指向栈顶压入值的可变原始指针 `*mut T`
    ///
    /// ## 栈操作过程
    ///
    /// ```text
    /// 压入前:                   压入后:
    /// ┌──────────────┐         ┌──────────────┐
    /// │              │         │   New Data   │ ← Returned pointer
    /// │              │         ├──────────────┤ ← New stack top
    /// │              │         │              │
    /// │ Stack Space  │         │ Original     │
    /// │              │         │ Content      │
    /// │              │         │              │
    /// └──────────────┘         └──────────────┘
    /// ↑ 栈顶                   ↑ 栈底
    /// ```
    ///
    /// ## Safety
    ///
    /// 此方法使用 `unsafe` 代码直接操作内存，但通过以下方式确保安全：
    /// - 栈顶地址由 `get_top()` 正确计算
    /// - 减去数据大小确保不会越界
    /// - 目标地址在已映射的内核栈范围内
    ///
    /// ## 使用场景
    ///
    /// - **保存上下文**: 在任务切换时保存 `TrapContext`
    /// - **传递参数**: 向新任务传递初始化参数
    /// - **临时存储**: 存储系统调用期间的临时数据
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 创建内核栈
    /// let kernel_stack = KernelStack::new(&pid_handle);
    ///
    /// // 压入陷阱上下文
    /// let trap_ctx = TrapContext::init_app_context(entry_point, user_sp);
    /// let ctx_ptr = kernel_stack.push_on_top(trap_ctx);
    ///
    /// // 现在可以通过 ctx_ptr 访问压入的数据
    /// unsafe {
    ///     (*ctx_ptr).sepc = new_entry_point;
    /// }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - **栈溢出风险**: 频繁压入大对象可能导致栈溢出
    /// - **生命周期**: 返回的指针的有效性与 `KernelStack` 实例相关
    /// - **并发安全**: 不支持多线程同时操作同一个内核栈
    #[allow(unused)]
    pub fn push_on_top<T>(&self, value: T) -> *mut T
    where
        T: Sized,
    {
        let kernel_stack_top = self.get_top();
        let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T;
        unsafe {
            *ptr_mut = value;
        }
        ptr_mut
    }

    /// 获取内核栈顶地址
    ///
    /// 返回当前进程内核栈的顶部虚拟地址。栈顶是栈空间的最高地址，
    /// 新数据从这里开始向下增长。
    ///
    /// ## Returns
    ///
    /// 内核栈顶的虚拟地址（`usize` 类型）
    ///
    /// ## 地址计算
    ///
    /// 栈顶地址通过以下公式计算：
    /// ```text
    /// stack_top = TRAMPOLINE - (KERNEL_STACK_SIZE + PAGE_SIZE) * pid
    /// ```
    ///
    /// 其中：
    /// - `TRAMPOLINE`: 蹦床页的起始地址（虚拟地址空间最高处）
    /// - `KERNEL_STACK_SIZE`: 单个内核栈的大小（通常 8KB）
    /// - `PAGE_SIZE`: 页面大小（通常 4KB），用作保护页
    /// - `pid`: 当前进程的 ID
    ///
    /// ## 使用场景
    ///
    /// - **栈指针初始化**: 设置任务切换时的栈指针
    /// - **数据压栈**: 计算压入数据后的新地址
    /// - **栈空间检查**: 验证栈使用情况和剩余空间
    /// - **调试诊断**: 输出栈地址信息进行问题排查
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let kernel_stack = KernelStack::new(&pid_handle);
    /// let stack_top = kernel_stack.get_top();
    ///
    /// println!("Kernel stack top: 0x{:x}", stack_top);
    ///
    /// // 压入数据并计算新的栈顶
    /// let data_ptr = kernel_stack.push_on_top(some_data);
    /// let new_top = stack_top - core::mem::size_of::<SomeData>();
    /// assert_eq!(data_ptr as usize, new_top);
    /// ```
    ///
    /// ## 性能特性
    ///
    /// - **O(1) 复杂度**: 地址计算是简单的算术运算
    /// - **无内存访问**: 不需要访问实际的物理内存
    /// - **缓存友好**: 频繁调用不会产生缓存开销
    pub fn get_top(&self) -> usize {
        let (_, kernel_stack_top) = kernel_stack_position(self.pid);
        kernel_stack_top
    }
}

/// `KernelStack` 的自动资源清理
///
/// 实现 `Drop` trait 为内核栈提供 RAII 风格的自动资源管理。当 `KernelStack`
/// 实例被销毁时，会自动清理对应的虚拟内存映射，防止内存泄漏。
///
/// ## 清理过程
///
/// 1. **地址计算**: 重新计算内核栈的底部虚拟地址
/// 2. **地址转换**: 将 usize 地址转换为 `VirtAddr` 类型
/// 3. **页面转换**: 将虚拟地址转换为虚拟页号 (VPN)
/// 4. **映射移除**: 从内核地址空间中移除整个内核栈区域的映射
/// 5. **物理页回收**: 底层的物理页面被自动回收到页面分配器
///
/// ## 自动触发时机
///
/// `drop()` 方法在以下情况下自动调用：
/// - **作用域结束**: `KernelStack` 变量离开作用域
/// - **显式销毁**: 调用 `drop(kernel_stack)` 显式销毁
/// - **重新赋值**: 变量被赋予新值时，旧值被销毁
/// - **容器清理**: 从 Vec 或其他容器中移除时
/// - **程序终止**: 程序结束时自动清理所有资源
///
/// ## 内存管理
///
/// ```text
/// 销毁前:                     销毁后:
/// ┌─────────────────┐         ┌─────────────────┐
/// │ Virtual Address │         │ Virtual Address │
/// │ Space           │         │ Space           │
/// │ ┌─────────────┐ │         │                 │
/// │ │ Stack       │ │   -->   │  (Mapping       │
/// │ │ Mapping     │ │         │   Removed)      │
/// │ └─────────────┘ │         │                 │
/// └─────────────────┘         └─────────────────┘
///       ↓                            
/// ┌─────────────────┐         ┌─────────────────┐
/// │ Physical Memory │         │ Physical Memory │
/// │ ┌─────────────┐ │         │ ┌─────────────┐ │
/// │ │ Allocated   │ │   -->   │ │ Free Pages  │ │
/// │ │ Pages       │ │         │ │             │ │
/// │ └─────────────┘ │         │ └─────────────┘ │
/// └─────────────────┘         └─────────────────┘
/// ```
///
/// ## 线程安全
///
/// 通过 `KERNEL_SPACE.exclusive_access()` 获取互斥访问权，确保在多线程环境下
/// 的内存管理操作是原子的和线程安全的。
///
/// ## 异常安全
///
/// 即使在 `drop()` 执行过程中发生 panic，内核地址空间的状态仍然保持一致，
/// 不会出现部分清理的情况。
///
/// ## 性能特性
///
/// - **延迟回收**: 物理页面回收可能是延迟的，具体取决于内存管理器实现
/// - **批量操作**: 移除整个区域的映射比逐页移除更高效
/// - **无额外分配**: 清理过程不需要额外的内存分配
///
/// ## 调试信息
///
/// 在调试模式下，可以通过内存管理器的调试接口验证内核栈是否被正确清理。
impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_position(self.pid);
        let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
        KERNEL_SPACE
            .exclusive_access()
            .remove_area_with_start_vpn(kernel_stack_bottom_va.into());
    }
}

/// 计算内核栈在虚拟地址空间中的位置
///
/// 根据进程 ID 计算对应内核栈的虚拟地址范围。内核栈在虚拟地址空间中从高地址向低地址排列，
/// 每个栈之间通过保护页分隔，防止栈溢出时影响其他进程的内核栈。
///
/// ## Arguments
///
/// * `app_id` - 进程/应用程序的 ID，用于确定内核栈在地址空间中的位置
///
/// ## Returns
///
/// 返回一个元组 `(bottom, top)`:
/// - `bottom`: 内核栈底部的虚拟地址（低地址）
/// - `top`: 内核栈顶部的虚拟地址（高地址）
///
/// ## 地址计算公式
///
/// ```text
/// stack_top = TRAMPOLINE - (KERNEL_STACK_SIZE + PAGE_SIZE) * app_id
/// stack_bottom = stack_top - KERNEL_STACK_SIZE
/// ```
///
/// 其中：
/// - `TRAMPOLINE`: 跳板页的起始地址，通常为用户地址空间的最高地址
/// - `KERNEL_STACK_SIZE`: 单个内核栈的大小（通常为 8KB）
/// - `PAGE_SIZE`: 页面大小（通常为 4KB），用作保护页
/// - `app_id`: 进程 ID，从 0 开始
///
/// ## 内存布局示意图
///
/// ```text
/// 高地址 TRAMPOLINE
///        ↓
/// ┌─────────────────────┐ ← app_id=0 的栈顶
/// │   Kernel Stack 0    │ ← 8KB
/// └─────────────────────┘ ← app_id=0 的栈底
/// ┌─────────────────────┐ ← 保护页 (4KB)
/// └─────────────────────┘
/// ┌─────────────────────┐ ← app_id=1 的栈顶
/// │   Kernel Stack 1    │ ← 8KB
/// └─────────────────────┘ ← app_id=1 的栈底
/// ┌─────────────────────┐ ← 保护页 (4KB)
/// └─────────────────────┘
///        ↓
///    (更多内核栈...)
///        ↓
/// 低地址
/// ```
///
/// ## 设计考虑
///
/// ### 保护页机制
/// 每个内核栈之间插入一个未映射的保护页，提供以下保护：
/// - **栈溢出检测**: 访问保护页会触发页面错误
/// - **隔离保护**: 防止一个进程的栈溢出影响其他进程
/// - **调试辅助**: 栈溢出时能快速定位问题
///
/// ### 地址分配策略
/// - **确定性布局**: 每个进程的内核栈位置完全由其 PID 决定
/// - **快速计算**: O(1) 时间复杂度，无需查表或搜索
/// - **内存对齐**: 所有地址都自然对齐到页边界
/// - **向下增长**: 符合大多数架构的栈增长方向
///
/// ## 使用示例
///
/// ```rust
/// // 计算进程 0 的内核栈位置
/// let (bottom, top) = kernel_stack_position(0);
/// println!("Process 0 kernel stack: 0x{:x} - 0x{:x}", bottom, top);
/// assert_eq!(top - bottom, KERNEL_STACK_SIZE);
///
/// // 验证相邻进程的栈有保护页分隔
/// let (bottom1, top1) = kernel_stack_position(1);
/// assert!(top1 + PAGE_SIZE <= bottom); // 保护页存在
/// ```
///
/// ## 限制和注意事项
///
/// - **地址空间限制**: 可分配的内核栈数量受虚拟地址空间大小限制
/// - **PID 范围**: 过大的 `app_id` 可能导致地址下溢或冲突
/// - **静态布局**: 内核栈大小在编译时确定，运行时无法调整
///
/// ## 性能特性
///
/// - **计算开销**: O(1) 时间复杂度，仅涉及简单算术运算
/// - **无内存访问**: 纯计算函数，不访问内存或全局状态
/// - **缓存友好**: 多次调用不会产生缓存未命中
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - (KERNEL_STACK_SIZE + PAGE_SIZE) * app_id;
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}
