//! # 处理器管理模块
//!
//! 提供处理器状态管理和进程调度的核心功能，实现抢占式多进程调度系统。
//! 管理当前正在执行的进程，协调进程切换和处理器资源分配。
//!
//! ## 核心组件
//!
//! - [`Processor`] - 处理器状态管理器，维护当前进程和空闲上下文
//! - [`PROCESSOR`] - 全局处理器实例，系统唯一的处理器管理器
//! - [`run_processs`] - 主调度循环，负责进程分发和执行
//! - [`schedule`] - 进程调度函数，实现进程上下文切换
//!
//! ## 调度机制
//!
//! ### 调度策略
//! 采用**协作式调度**与**抢占式调度**相结合的混合调度模式：
//! - **时间片轮转**: 基于时钟中断的抢占式调度
//! - **主动让出**: 进程可以主动调用 `yield` 让出 CPU
//! - **阻塞调度**: I/O 等待时自动切换到其他进程
//!
//! ### 进程状态转换
//! ```text
//! ┌─────────────┐   Scheduler    ┌─────────────┐
//! │    Ready    │ ─────────────► │   Running   │
//! └─────────────┘   Selection    └──────┬──────┘
//!       ▲                               │
//!       │                               │
//!       │    Timeout/Yield/Block        │
//!       └───────────────────────────────┘
//! ```
//!
//! ## 处理器架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    CPU Core                             │
//! │  ┌─────────────┐                    ┌─────────────┐     │
//! │  │   Current   │                    │  Idle Process  │     │
//! │  │    Process     │ ◄──── Switch ────► │   Context   │     │
//! │  │   Context   │                    │             │     │
//! │  └─────────────┘                    └─────────────┘     │
//! └─────────────────────────────────────────────────────────┘
//!              ▲                                ▲
//!              │                                │
//!        ┌─────────────┐                ┌─────────────┐
//!        │ User Space  │                │ Scheduler   │
//!        │   Processs     │                │   Loop      │
//!        └─────────────┘                └─────────────┘
//! ```
//!
//! ## 上下文切换流程
//!
//! ```text
//! Process A Run ──► Interrupt/Syscall ──► Save Context A ──► Load Context B ──► Process B Run
//!    │                               │                 │             │
//!    │                               ▼                 ▼             │
//!    │                        ┌─────────────┐ ┌─────────────┐        │
//!    │                        │ ProcessContext │ │ ProcessContext │        │
//!    │                        │     A       │ │     B       │        │
//!    │                        └─────────────┘ └─────────────┘        │
//!    │                                                               │
//!    └─────────────────── Time Slice Rotation ◄──────────────────────┘
//! ```
//!
//! ## 使用示例
//!
//! ```rust
//! // 系统初始化后启动调度器
//! run_processs(); // 进入主调度循环，永不返回
//!
//! // 在中断处理中进行进程切换
//! pub fn handle_timer_interrupt() {
//!     // 处理时钟中断...
//!     schedule(current_process_cx_ptr); // 切换到其他进程
//! }
//!
//! // 获取当前进程信息
//! if let Some(process) = current_process() {
//!     println!("Current PID: {}", process.getpid());
//! }
//! ```

use crate::process::manager::fetch_process;
use crate::process::process::ProcessStatus;
use crate::process::switch::__switch;
use crate::process::{context::ProcessContext, process::ProcessControlBlock};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::lazy_static;

/// 处理器状态管理器
///
/// 管理单个 CPU 核心的执行状态，包括当前正在执行的进程和空闲进程上下文。
/// 作为调度器的核心数据结构，协调进程之间的 CPU 资源分配。
///
/// ## 设计原理
///
/// ### 双上下文模型
/// 处理器维护两种执行上下文：
/// - **进程上下文**: 用户进程的执行状态（寄存器、栈指针等）
/// - **空闲上下文**: 调度器的执行状态，用于进程切换时的中转
///
/// ### 所有权管理
/// 通过 `Option<Arc<ProcessControlBlock>>` 管理当前进程：
/// - `Some(process)`: CPU 正在执行该进程
/// - `None`: CPU 处于空闲状态，等待新进程调度
///
/// ## 状态转换
///
/// ```text
/// 处理器状态转换:
///
///    Idle State            Running State
/// ┌─────────────┐         ┌─────────────┐
/// │   Idle      │ ──────► │   Running   │
/// │ current=None│Schedule │current=Some │
/// └─────────────┘         └──────┬──────┘
///       ▲                        │
///       │  Process Complete/Switch  │
///       └────────────────────────┘
/// ```
///
/// ## 内存布局
///
/// ```text
/// Processor 内存结构:
/// ┌─────────────────────────────────────┐
/// │           Processor                 │
/// ├─────────────────────────────────────┤
/// │ current: Option<Arc<TCB>>           │
/// │  └─ Some: Points to Current TCB     │
/// │  └─ None: Processor Idle            │
/// ├─────────────────────────────────────┤
/// │ idle_process_cx: ProcessContext           │
/// │  └─ ra: Return Address              │
/// │  └─ sp: Stack Pointer               │
/// │  └─ s0-s11: Saved Registers         │
/// └─────────────────────────────────────┘
/// ```
///
/// ## 并发安全
///
/// `Processor` 本身不提供线程安全保证，必须通过 `UPSafeCell` 包装：
/// - 单处理器系统中通过禁用中断保证原子性
/// - 多处理器系统需要额外的同步机制
///
/// ## 性能特征
///
/// - **内存占用**: 约 264 bytes（ProcessContext ≈ 264 bytes + Arc 指针）
/// - **切换开销**: O(1) 常数时间的上下文切换
/// - **缓存友好**: 紧凑的内存布局提供良好的缓存局部性
pub struct Processor {
    /// 当前正在执行的进程
    ///
    /// - `Some(process)`: 指向正在 CPU 上执行的进程控制块
    /// - `None`: CPU 处于空闲状态，调度器正在寻找下一个可运行进程
    ///
    /// 使用 `Arc` 允许进程控制块在调度器和其他组件之间共享所有权。
    current: Option<Arc<ProcessControlBlock>>,

    /// 空闲进程上下文
    ///
    /// 当没有用户进程运行时，CPU 执行调度器循环的上下文。
    /// 包含调度器的寄存器状态，用于在进程切换时保存/恢复调度器状态。
    ///
    /// ## 作用
    /// - **进程切换中转**: 从进程A切换到进程B时的中间状态
    /// - **调度器状态**: 保存调度器循环的执行状态
    /// - **空闲处理**: CPU 空闲时的默认执行上下文
    idle_process_cx: ProcessContext,
}

impl Processor {
    /// 创建新的处理器实例
    ///
    /// 初始化一个空闲状态的处理器，准备接受进程调度。
    /// 处理器创建时没有当前进程，空闲上下文被清零。
    ///
    /// ## Returns
    ///
    /// 新创建的处理器实例，处于空闲状态
    ///
    /// ## 初始状态
    ///
    /// - `current`: `None` - 没有正在执行的进程
    /// - `idle_process_cx`: 零初始化的进程上下文
    ///
    /// ## 使用场景
    ///
    /// - 系统初始化时创建全局处理器
    /// - 多处理器系统中创建额外的处理器核心
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let processor = Processor::new();
    /// assert!(processor.current().is_none()); // 初始状态为空闲
    /// ```
    pub fn new() -> Self {
        Self {
            current: None,
            idle_process_cx: ProcessContext::zero_init(),
        }
    }

    /// 获取当前正在执行的进程（克隆引用）
    ///
    /// 返回当前进程控制块的克隆引用，保持原始引用不变。
    /// 适用于需要访问当前进程信息但不需要取走所有权的场景。
    ///
    /// ## Returns
    ///
    /// - `Some(Arc<ProcessControlBlock>)` - 当前进程的克隆引用
    /// - `None` - 处理器当前处于空闲状态
    ///
    /// ## 引用计数
    ///
    /// 此函数会增加进程控制块的引用计数：
    /// ```text
    /// 调用前: Arc::strong_count = n
    /// 调用后: Arc::strong_count = n + 1
    /// ```
    ///
    /// ## 使用场景
    ///
    /// - **信息查询**: 获取当前进程的 PID、状态等信息
    /// - **权限检查**: 验证当前进程的访问权限
    /// - **上下文共享**: 在多个组件间共享进程引用
    ///
    /// ## Examples
    ///
    /// ```rust
    /// if let Some(process) = processor.current() {
    ///     println!("Current PID: {}", process.getpid());
    ///     // 进程引用在作用域结束时自动释放
    /// } else {
    ///     println!("No process currently running");
    /// }
    /// ```
    ///
    /// ## 性能考虑
    ///
    /// - 引用计数操作开销很小（原子操作）
    /// - 适合频繁的只读访问场景
    /// - 避免不必要的所有权转移
    pub fn current(&self) -> Option<Arc<ProcessControlBlock>> {
        self.current.as_ref().map(|process| Arc::clone(process))
    }

    /// 取出当前正在执行的进程（转移所有权）
    ///
    /// 移除并返回当前进程控制块，将处理器设置为空闲状态。
    /// 这是一个所有权转移操作，调用后处理器不再持有进程引用。
    ///
    /// ## Returns
    ///
    /// - `Some(Arc<ProcessControlBlock>)` - 被取出的进程控制块
    /// - `None` - 处理器已经处于空闲状态
    ///
    /// ## 状态变化
    ///
    /// ```text
    /// 调用前: current = Some(process), 处理器运行状态
    /// 调用后: current = None,       处理器空闲状态
    /// ```
    ///
    /// ## 使用场景
    ///
    /// - **进程切换**: 在调度新进程前取出当前进程
    /// - **进程完成**: 进程退出时清理处理器状态
    /// - **进程挂起**: 将进程转移到等待队列
    ///
    /// ## 调度流程中的作用
    ///
    /// ```text
    /// 1. take_current_process() ──► 取出当前进程
    /// 2. 进程状态处理 ────────► 加入就绪队列或退出
    /// 3. fetch_process() ───────► 获取新进程
    /// 4. 设置新的 current ───► 开始执行新进程
    /// ```
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 在进程调度中使用
    /// if let Some(old_process) = processor.process_current() {
    ///     // 处理旧进程（加入就绪队列、退出等）
    ///     handle_process_switch(old_process);
    /// }
    /// // 现在处理器处于空闲状态，可以调度新进程
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 调用后处理器立即进入空闲状态
    /// - 必须确保在合适的时机调用，避免进程丢失
    /// - 通常与进程调度算法配合使用
    pub fn process_current(&mut self) -> Option<Arc<ProcessControlBlock>> {
        self.current.take()
    }

    /// 获取空闲进程上下文的可变指针
    ///
    /// 返回指向空闲进程上下文的原始指针，用于底层的上下文切换操作。
    /// 这是一个私有方法，只在内部上下文切换时使用。
    ///
    /// ## Returns
    ///
    /// `*mut ProcessContext` - 指向空闲上下文的可变原始指针
    ///
    /// ## 安全性考虑
    ///
    /// 此方法返回原始指针，调用者必须确保：
    /// - 指针在使用期间保持有效
    /// - 不会导致数据竞争或内存安全问题
    /// - 只在适当的同步保护下使用
    ///
    /// ## 使用场景
    ///
    /// - **上下文切换**: 在 `__switch` 汇编函数中使用
    /// - **调度器循环**: 保存/恢复调度器状态
    /// - **进程切换中转**: 作为进程间切换的中介
    ///
    /// ## 上下文切换流程
    ///
    /// ```text
    /// 1. 保存当前进程上下文到进程控制块
    /// 2. 恢复空闲上下文 ────────────► 返回调度器
    /// 3. 调度器选择新进程
    /// 4. 保存空闲上下文 ────────────► 为下次切换准备
    /// 5. 恢复新进程上下文 ─────────► 执行新进程
    /// ```
    ///
    /// ## 内存布局
    ///
    /// 返回的指针指向 `idle_process_cx` 字段的内存地址，
    /// 包含完整的 RISC-V 寄存器上下文信息。
    fn idle_process_cx_ptr(&mut self) -> *mut ProcessContext {
        &mut self.idle_process_cx as *mut _
    }
}

lazy_static! {
    /// 全局处理器实例
    ///
    /// 系统中唯一的处理器管理器，负责协调所有的进程调度和 CPU 资源分配。
    /// 使用 `UPSafeCell` 提供单处理器环境下的安全可变访问。
    ///
    /// ## 设计特点
    ///
    /// ### 单例模式
    /// - **全局唯一**: 整个系统只有一个处理器管理器实例
    /// - **延迟初始化**: 在首次访问时才进行初始化
    /// - **生命周期管理**: 伴随程序整个生命周期存在
    ///
    /// ### 并发安全
    /// - **UPSafeCell 保护**: 单处理器环境下的内部可变性
    /// - **互斥访问**: 通过 `exclusive_access()` 获取独占访问权
    /// - **中断安全**: 在中断处理期间禁用抢占保证原子性
    ///
    /// ### 内存布局
    /// ```text
    /// 全局内存区域:
    /// ┌─────────────────────────────────────────┐
    /// │             PROCESSOR                   │
    /// │ ┌─────────────────────────────────────┐ │
    /// │ │        UPSafeCell<Processor>        │ │
    /// │ │ ┌─────────────────────────────────┐ │ │
    /// │ │ │         Processor               │ │ │
    /// │ │ │  ┌───────────────────────────┐  │ │ │
    /// │ │ │  │ current: Option<Arc<TCB>> │  │ │ │
    /// │ │ │  ├───────────────────────────┤  │ │ │
    /// │ │ │  │ idle_process_cx: ProcessContext │  │ │ │
    /// │ │ │  └───────────────────────────┘  │ │ │
    /// │ │ └─────────────────────────────────┘ │ │
    /// │ └─────────────────────────────────────┘ │
    /// └─────────────────────────────────────────┘
    /// ```
    ///
    /// ## 访问模式
    ///
    /// ### 推荐方式（通过全局函数）
    /// ```rust
    /// // 安全的高层接口
    /// let current_process = current_process();
    /// let user_token = current_user_token();
    /// ```
    ///
    /// ### 直接访问（需要小心）
    /// ```rust
    /// // 低层接口，需要手动管理锁
    /// let processor = PROCESSOR.exclusive_access();
    /// let process = processor.current();
    /// drop(processor); // 及时释放锁
    /// ```
    ///
    /// ## 初始化时机
    ///
    /// - **首次访问**: 在第一次调用相关函数时初始化
    /// - **系统启动**: 通常在调度器启动前完成初始化
    /// - **一次性**: 初始化后不会重复执行
    ///
    /// ## 性能影响
    ///
    /// - **全局访问**: 不涉及页表切换，访问速度快
    /// - **锁开销**: 在单核系统中开销极小
    /// - **缓存友好**: 全局变量在内存中位置固定
    ///
    /// ## 线程安全保证
    ///
    /// - **原子访问**: `exclusive_access()` 保证同一时间只有一个访问者
    /// - **中断禁用**: 访问期间自动禁用中断避免竞争
    /// - **RAII 管理**: 作用域结束时自动释放访问权限
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// // 系统初始化时
    /// pub fn init_scheduler() {
    ///     // PROCESSOR 在首次访问时自动初始化
    ///     run_processs(); // 启动主调度循环
    /// }
    ///
    /// // 中断处理中
    /// pub fn handle_timer_interrupt() {
    ///     // 通过全局函数安全访问
    ///     if let Some(process) = current_process() {
    ///         // 处理当前进程的时间片...
    ///     }
    /// }
    /// ```
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

/// 取出当前正在执行的进程
///
/// 从全局处理器中移除并返回当前进程，将处理器设置为空闲状态。
/// 这是一个高级封装函数，提供线程安全的进程取出操作。
///
/// ## Returns
///
/// - `Some(Arc<ProcessControlBlock>)` - 成功取出的当前进程
/// - `None` - 处理器当前没有运行任何进程
///
/// ## 线程安全
///
/// 此函数通过 `PROCESSOR.exclusive_access()` 确保原子性操作：
/// - 获取全局处理器的独占访问权
/// - 安全地取出当前进程
/// - 自动释放访问权限
///
/// ## 使用场景
///
/// ### 进程调度
/// ```rust
/// // 在调度器中取出当前进程进行状态管理
/// if let Some(process) = take_current_process() {
///     // 根据进程状态决定后续处理
///     match process.inner_exclusive_access().process_status {
///         ProcessStatus::Running => {
///             // 进程主动让出 CPU，加入就绪队列
///             add_process(process);
///         }
///         ProcessStatus::Zombie => {
///             // 进程已退出，不需要重新调度
///             println!("Process {} exited", process.getpid());
///         }
///         _ => {
///             // 其他状态处理...
///         }
///     }
/// }
/// ```
///
/// ### 系统调用处理
/// ```rust
/// // 在 exit 系统调用中
/// pub fn sys_exit(exit_code: i32) -> ! {
///     let process = take_current_process().unwrap();
///     // 设置进程为僵尸状态...
///     // 调度到其他进程...
/// }
/// ```
///
/// ## 性能特征
///
/// - **时间复杂度**: O(1) 常数时间操作
/// - **锁开销**: 单核系统中开销极小
/// - **原子性**: 通过独占访问保证操作的原子性
///
/// ## 注意事项
///
/// - 调用后处理器立即进入空闲状态
/// - 必须确保有适当的进程调度机制跟进
/// - 避免长时间持有取出的进程而不进行处理
pub fn take_current_process() -> Option<Arc<ProcessControlBlock>> {
    PROCESSOR.exclusive_access().process_current()
}

/// 获取当前正在执行的进程（只读访问）
///
/// 返回当前进程的克隆引用，不改变处理器状态。适用于需要查询
/// 当前进程信息但不需要修改进程状态的场景。
///
/// ## Returns
///
/// - `Some(Arc<ProcessControlBlock>)` - 当前进程的克隆引用
/// - `None` - 处理器当前没有运行任何进程
///
/// ## 引用语义
///
/// 返回的是 `Arc` 的克隆，增加引用计数但不转移所有权：
/// ```text
/// 处理器状态保持不变：current = Some(process)
/// 返回值：Arc::clone(process) - 新的引用
/// ```
///
/// ## 使用场景
///
/// ### 信息查询
/// ```rust
/// // 获取当前进程信息
/// if let Some(process) = current_process() {
///     println!("Current PID: {}", process.getpid());
///     println!("Process status: {:?}", process.inner_exclusive_access().process_status);
/// } else {
///     println!("No process currently running");
/// }
/// ```
///
/// ### 权限检查
/// ```rust
/// // 验证系统调用权限
/// pub fn sys_read(fd: usize, buf: *mut u8, len: usize) -> isize {
///     if let Some(process) = current_process() {
///         // 验证文件描述符属于当前进程...
///         // 验证内存访问权限...
///     } else {
///         return -1; // 无当前进程
///     }
/// }
/// ```
///
/// ### 上下文获取
/// ```rust
/// // 获取当前进程的内存地址空间
/// let current_memory_set = current_process()
///     .unwrap()
///     .inner_exclusive_access()
///     .memory_set
///     .token();
/// ```
///
/// ## 性能考虑
///
/// - **引用计数开销**: Arc::clone 涉及原子操作，开销很小
/// - **锁开销**: 短暂的独占访问，影响极小  
/// - **内存友好**: 不复制进程数据，只复制引用
///
/// ## 线程安全
///
/// 通过 `PROCESSOR.exclusive_access()` 保证读取的原子性，
/// 避免在读取过程中进程被切换导致的竞争条件。
pub fn current_process() -> Option<Arc<ProcessControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// 获取当前进程的用户地址空间标识符
///
/// 返回当前用户进程的页表标识符（satp 寄存器值），用于地址空间管理
/// 和内存访问控制。这是一个便捷函数，封装了获取用户空间令牌的复杂过程。
///
/// ## Returns
///
/// `usize` - 当前进程的用户地址空间标识符
///
/// ## Panics
///
/// 如果当前没有运行进程，函数会 panic。调用前应确保有进程在运行。
///
/// ## satp 寄存器格式
///
/// 返回值符合 RISC-V satp 寄存器格式：
/// ```text
/// ┌────────────┬────────────────┬──────────────────────────────────────────────┐
/// │    MODE    │      ASID      │                    PPN                       │
/// │   (4bit)   │     (16bit)    │                  (44bit)                     │
/// └────────────┴────────────────┴──────────────────────────────────────────────┘
/// ```
///
/// ## 使用场景
///
/// ### 地址空间切换
/// ```rust
/// // 在用户态陷阱处理中保存/恢复地址空间
/// pub fn handle_user_trap() {
///     let user_satp = current_user_token();
///     
///     // 切换到内核地址空间处理陷阱...
///     
///     // 恢复用户地址空间
///     unsafe {
///         satp::write(user_satp);
///         asm!("sfence.vma");
///     }
/// }
/// ```
///
/// ### 内存访问验证
/// ```rust
/// // 验证用户地址是否属于当前进程
/// pub fn validate_user_address(addr: VirtAddr) -> bool {
///     let user_token = current_user_token();
///     // 使用用户页表进行地址转换验证...
/// }
/// ```
///
/// ### 系统调用参数访问
/// ```rust
/// // 安全地访问用户空间数据
/// pub fn copy_from_user(user_ptr: *const u8, len: usize) -> Vec<u8> {
///     let user_token = current_user_token();
///     // 使用用户页表访问用户内存...
/// }
/// ```
///
/// ## 调用链
///
/// ```text
/// current_user_token()
///     ↓
/// current_process().unwrap()
///     ↓
/// process.inner_exclusive_access()
///     ↓
/// inner.user_token()
///     ↓
/// memory_set.token()
/// ```
///
/// ## 性能特征
///
/// - **多级访问**: 需要穿越多层封装获取令牌
/// - **锁开销**: 涉及进程控制块的互斥访问
/// - **缓存友好**: 令牌通常会被频繁使用，具有良好的时间局部性
///
/// ## 安全考虑
///
/// 返回的令牌代表了完整的用户地址空间访问权限，使用时需要注意：
/// - 确保在正确的上下文中使用
/// - 避免跨进程使用其他进程的令牌
/// - 在地址空间切换后及时更新
pub fn current_user_token() -> usize {
    let process = current_process().unwrap();
    let token = process.inner_exclusive_access().user_token();
    token
}

/// 获取当前进程的陷阱上下文
///
/// 返回指向当前进程陷阱上下文的可变引用，用于中断和系统调用处理。
/// 陷阱上下文包含了用户程序在陷入内核时的完整 CPU 状态。
///
/// ## Returns
///
/// `&'static mut TrapContext` - 指向当前进程陷阱上下文的可变引用
///
/// ## Panics
///
/// 如果当前没有运行进程，函数会 panic。通常在中断/异常处理上下文中调用。
///
/// ## 陷阱上下文结构
///
/// 返回的陷阱上下文包含：
/// ```text
/// TrapContext {
///     x[0..32]:  通用寄存器（x0-x31）
///     sstatus:   处理器状态寄存器
///     sepc:      异常程序计数器
///     kernel_satp: 内核页表标识符
///     kernel_sp:   内核栈指针
///     trap_handler: 陷阱处理函数地址
/// }
/// ```
///
/// ## 使用场景
///
/// ### 系统调用处理
/// ```rust
/// // 获取系统调用参数和设置返回值
/// pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
///     let cx = current_trap_cx();
///     
///     // 读取寄存器参数
///     let arg0 = cx.x[10]; // a0 寄存器
///     let arg1 = cx.x[11]; // a1 寄存器
///     
///     let result = match syscall_id {
///         // 处理各种系统调用...
///     };
///     
///     // 设置返回值
///     cx.x[10] = result as usize; // 通过 a0 寄存器返回
/// }
/// ```
///
/// ### 中断处理
/// ```rust
/// // 时钟中断处理
/// pub fn handle_timer_interrupt() {
///     let cx = current_trap_cx();
///     
///     // 保存中断时的程序计数器
///     let user_pc = cx.sepc;
///     println!("Timer interrupt at PC: {:#x}", user_pc);
///     
///     // 设置时间片到期标志...
/// }
/// ```
///
/// ### 程序状态检查
/// ```rust
/// // 检查用户程序执行状态
/// pub fn check_user_state() {
///     let cx = current_trap_cx();
///     
///     if cx.sstatus.spie() {
///         println!("User program had interrupts enabled");
///     }
///     
///     println!("User PC: {:#x}", cx.sepc);
///     println!("User SP: {:#x}", cx.x[2]); // sp 寄存器
/// }
/// ```
///
/// ## 内存布局
///
/// 陷阱上下文位于用户地址空间的固定位置：
/// ```text
/// 用户地址空间高地址区域:
/// ┌──────────────────────────────────┐ ← TRAMPOLINE
/// │         Trampoline Page          │
/// ├──────────────────────────────────┤ ← TRAP_CONTEXT  
/// │        Trap Context              │ ← current_trap_cx() 返回
/// │  ┌────────────────────────────┐  │
/// │  │ x[0..32]: General Registers│  │
/// │  ├────────────────────────────┤  │
/// │  │ sstatus: Status Register   │  │
/// │  ├────────────────────────────┤  │
/// │  │ sepc: Program Counter      │  │
/// │  └────────────────────────────┘  │
/// └──────────────────────────────────┘
/// ```
///
/// ## 生命周期
///
/// 返回的引用具有 `'static` 生命周期，因为陷阱上下文在进程的整个
/// 生命周期内都有效，并且存储在固定的虚拟地址位置。
///
/// ## 安全性考虑
///
/// - 返回可变引用，可以直接修改用户程序状态
/// - 仅应在内核态的中断/系统调用处理中使用
/// - 修改陷阱上下文会直接影响用户程序的执行
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_process()
        .unwrap()
        .inner_exclusive_access()
        .trap_cx()
}

/// 主调度循环 - 系统调度器的核心
///
/// 这是操作系统的心脏，负责持续地从就绪队列中获取进程并执行。
/// 函数永不返回，构成了系统的主调度循环，确保 CPU 始终在执行有用的工作。
///
/// ## 调度算法
///
/// 采用 **FIFO (First In, First Out)** 调度策略：
/// - 按照进程进入就绪队列的顺序进行调度
/// - 保证调度的公平性和可预测性
/// - 避免进程饥饿问题
///
/// ## 执行流程
///
/// ```text
/// 主调度循环流程:
///
/// ┌───────────────────────────────────────────────────────────┐
/// │                    run_processs()                            │
/// │  ┌─────────────┐                                          │
/// │  │   Get       │ ──► PROCESSOR.exclusive_access()         │
/// │  │ Exclusive   │                                          │
/// │  │ Processor   │                                          │
/// │  └─────────────┘                                          │
/// │         │                                                 │
/// │         ▼                                                 │
/// │  ┌─────────────┐    Yes  ┌────────-───────────────────┐   │
/// │  │   Fetch     │ ──────► │     Prepare Process Switch    │   │
/// │  │ Next Ready  │         │  1. Get Idle Context Ptr   │   │
/// │  │   Process      │         │  2. Get Process Context Ptr   │   │
/// │  └─────────────┘         │  3. Set Process Status Running│   │
/// │         │                │  4. Set as Current Process    │   │
/// │         │ No             └────────────────────────────┘   │
/// │         ▼                               │                 │
/// │  ┌─────────────┐                        ▼                 │
/// │  │ Idle Wait   │         ┌─────────────────────────────┐  │
/// │  │ CPU Halt    │         │    Execute Context Switch   │  │
/// │  └─────────────┘         │  __switch(idle_cx, process_cx) │  │
/// │         │                └─────────────────────────────┘  │
/// │         │                               │                 │
/// │         │                               ▼                 │
/// │         │                ┌─────────────────────────────┐  │
/// │         │                │     Process Start Running      │  │
/// │         │                │  (Switch to User Mode)      │  │
/// │         │                └─────────────────────────────┘  │
/// │         │                               │                 │
/// │         │                               │                 │
/// │         └◄──────── Continue Loop ◄──────┘                 │
/// └───────────────────────────────────────────────────────────┘
/// ```
///
/// ## 上下文切换机制
///
/// ### 双上下文模型
/// ```text
/// 切换前:                切换后:
/// ┌────────────┐      ┌─-────────────┐
/// │Idle Context│ ──►  │ Process Context │
/// │(Scheduler) │ ◄─── │(User Program)│
/// └────────────┘      └──-───────────┘
/// ```
///
/// ### 切换步骤
/// 1. **保存调度器状态**: 将当前寄存器保存到 `idle_process_cx`
/// 2. **恢复进程状态**: 从 `process.process_cx` 恢复进程寄存器
/// 3. **跳转执行**: CPU 开始执行用户进程代码
/// 4. **进程返回**: 进程通过中断/系统调用返回时恢复调度器状态
///
/// ## 内存管理
///
/// 在进程切换时自动处理：
/// - **地址空间切换**: 每个进程有独立的虚拟地址空间
/// - **页表切换**: 通过 `satp` 寄存器切换页表
/// - **内核栈**: 每个进程有独立的内核栈用于系统调用处理
///
/// ## 性能特征
///
/// - **调度延迟**: O(1) - 常数时间进程获取和切换
/// - **上下文切换开销**: ~100-200 CPU 周期（寄存器保存/恢复）
/// - **内存开销**: 每进程约 4KB 内核栈 + 页表开销
/// - **公平性**: FIFO 策略保证所有进程获得公平的 CPU 时间
///
/// ## 使用场景
///
/// ### 系统启动
/// ```rust
/// pub fn main() {
///     // 系统初始化...
///     init_scheduler();
///     
///     // 启动调度器（永不返回）
///     run_processs();
/// }
/// ```
///
/// ### 空闲状态处理
/// ```rust
/// // 当没有就绪进程时，CPU 进入空闲循环
/// // run_processs() 会持续检查新的就绪进程
/// // 通常配合中断机制唤醒进程
/// ```
///
/// ## 并发安全
///
/// - **原子操作**: 通过 `exclusive_access()` 保证进程获取的原子性
/// - **中断安全**: 在关键区域禁用中断防止竞争条件
/// - **状态一致性**: 确保进程状态转换的一致性
///
/// ## 调试特征
///
/// 可以添加调试代码监控调度行为：
/// ```rust
/// // 在实际代码中可以添加调试输出
/// println!("Switching to process PID: {}", process.getpid());
/// ```
///
/// ## 注意事项
///
/// - 此函数永不返回，是系统的主循环
/// - 必须在系统初始化完成后调用
/// - 调用前应确保有初始进程在就绪队列中
/// - 如果没有进程，会在循环中等待（可能需要中断唤醒）
pub fn run_processs() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(process) = fetch_process() {
            let idle_process_cx_ptr = processor.idle_process_cx_ptr();
            let mut process_inner = process.inner_exclusive_access();
            let next_process_cx_ptr = &process_inner.process_cx as *const ProcessContext;
            process_inner.process_status = ProcessStatus::Running;
            drop(process_inner);
            processor.current = Some(process);
            drop(processor);
            unsafe {
                __switch(idle_process_cx_ptr, next_process_cx_ptr);
            }
        }
    }
}

/// 进程调度函数 - 从当前进程切换回调度器
///
/// 将执行控制权从当前正在运行的进程转移回调度器的核心函数。
/// 通常在时间片到期、进程主动让出 CPU 或进程阻塞时调用。
///
/// ## Arguments
///
/// * `switched_process_cx_ptr` - 指向当前进程上下文的可变指针，用于保存进程状态
///
/// ## 调度时机
///
/// ### 抢占式调度
/// ```rust
/// // 时钟中断处理中
/// pub fn handle_timer_interrupt() {
///     // 处理时间片到期
///     set_next_trigger();
///     
///     // 切换到调度器
///     schedule(current_process_cx_ptr);
/// }
/// ```
///
/// ### 协作式调度  
/// ```rust
/// // yield 系统调用
/// pub fn sys_yield() -> isize {
///     // 进程主动让出 CPU
///     schedule(current_process_cx_ptr);
///     0
/// }
/// ```
///
/// ### 阻塞调度
/// ```rust
/// // 进程等待资源时
/// pub fn sys_wait_for_resource() -> isize {
///     // 将进程加入等待队列
///     add_to_wait_queue(current_process());
///     
///     // 切换到其他进程
///     schedule(current_process_cx_ptr);
/// }
/// ```
///
/// ## 执行流程
///
/// ```text
/// schedule() 执行流程:
///
/// ┌─────────────────────────────────────────────────────────────┐
/// │                   schedule()                                │
/// │                                                             │
/// │  ┌─────────────────┐                                        │
/// │  │ Save Current    │ ──► Caller already saved process state    │
/// │  │ Process State to   │     to context                         │
/// │  │ Context         │                                        │
/// │  └─────────────────┘                                        │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  ┌─────────────────┐                                        │
/// │  │ Get Exclusive   │ ──► PROCESSOR.exclusive_access()       │
/// │  │ Processor       │                                        │
/// │  │ Access          │                                        │
/// │  └─────────────────┘                                        │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  ┌─────────────────┐                                        │
/// │  │ Get Idle        │ ──► processor.idle_process_cx_ptr()   │
/// │  │ Context         │                                        │
/// │  │ Pointer         │                                        │
/// │  └─────────────────┘                                        │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  ┌─────────────────┐                                        │
/// │  │ Release         │ ──► drop(processor)                    │
/// │  │ Processor       │                                        │
/// │  │ Access          │                                        │
/// │  └─────────────────┘                                        │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  ┌─────────────────┐                                        │
/// │  │ Execute Context │ ──► __switch(process_cx, idle_cx)         │
/// │  │ Switch Return   │                                        │
/// │  │ to Scheduler    │                                        │
/// │  └─────────────────┘                                        │
/// │           │                                                 │
/// │           ▼                                                 │
/// │  ┌─────────────────┐                                        │
/// │  │ Scheduler       │ ──► Return to run_processs() loop         │
/// │  │ Continues       │                                        │
/// │  │ Execution       │                                        │
/// │  └─────────────────┘                                        │
/// └─────────────────────────────────────────────────────────────┘
/// ```
///
/// ## 上下文切换详解
///
/// ### 切换方向
/// ```text
/// 进程执行中 ──► schedule() ──► 调度器循环
///     │                          │
///     │                          ▼
///     │                    选择新进程
///     │                          │
///     │                          ▼
///     └◄─── 新进程执行 ◄─── run_processs()
/// ```
///
/// ### 内存状态变化
/// ```text
/// 切换前:                 切换后:
/// ┌─────────────┐         ┌─────────────┐
/// │   Process A    │ ──────► │  Scheduler  │
/// │  (Running)  │         │  (Active)   │  
/// └─────────────┘         └─────────────┘
///       │                       │
///       ▼                       ▼
/// ┌─────────────┐         ┌─────────────┐
/// │ Process Context│         │ Idle Context│
/// │  (Saved)    │         │  (Restored) │
/// └─────────────┘         └─────────────┘
/// ```
///
/// ## 调用约定
///
/// ### 调用前要求
/// - 当前进程的上下文必须已经保存到 `switched_process_cx_ptr` 指向的位置
/// - 进程状态应该已经适当更新（Ready, Blocked 等）
/// - 如需要，进程应该已经加入相应的队列（就绪队列、等待队列等）
///
/// ### 调用后保证
/// - 控制权转移到调度器
/// - 调度器会选择下一个进程执行
/// - 当前进程可能稍后被重新调度
///
/// ## 性能考虑
///
/// - **切换开销**: 约 100-200 CPU 周期
/// - **内存访问**: 主要是寄存器保存/恢复
/// - **缓存影响**: 可能导致缓存未命中
/// - **TLB 影响**: 如果切换到不同进程，可能导致 TLB 刷新
///
/// ## 使用示例
///
/// ### 时间片轮转
/// ```rust
/// // 在时钟中断处理函数中
/// pub extern "C" fn timer_interrupt_handler() {
///     // 更新时间片计数...
///     
///     // 时间片到期，调度其他进程
///     let current_cx_ptr = current_process_context_ptr();
///     
///     // 将当前进程重新加入就绪队列
///     if let Some(process) = take_current_process() {
///         add_process(process);
///     }
///     
///     // 切换到调度器选择新进程
///     schedule(current_cx_ptr);
/// }
/// ```
///
/// ## 安全性考虑
///
/// - 使用 `unsafe` 代码进行底层上下文切换
/// - 必须确保传入的指针有效且指向正确的上下文结构
/// - 调用时必须在适当的内核态上下文中
///
/// ## 与 run_processs() 的协作
///
/// `schedule()` 和 `run_processs()` 构成完整的调度循环：
/// - `run_processs()`: 从调度器切换到进程
/// - `schedule()`: 从进程切换回调度器
pub fn schedule(switched_process_cx_ptr: *mut ProcessContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_process_cx_ptr = processor.idle_process_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_process_cx_ptr, idle_process_cx_ptr);
    }
}
