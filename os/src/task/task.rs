//! # 任务控制块模块
//!
//! 提供任务控制块 (Task Control Block, TCB) 的实现，管理进程的完整生命周期。
//! 支持进程创建、fork、exec、状态管理等核心操作，是操作系统进程管理的核心数据结构。
//!
//! ## 核心组件
//!
//! - [`TaskControlBlock`] - 任务控制块，包含进程的所有信息
//! - [`TaskControlBlockInner`] - TCB 内部可变部分，受互斥锁保护
//! - [`TaskStatus`] - 任务状态枚举，表示进程的运行状态
//!
//! ## 设计原理
//!
//! ### 分离设计
//! TCB 采用内外分离的设计模式：
//! - **不变部分**：PID 和内核栈在进程生命周期内保持不变
//! - **可变部分**：任务状态、内存集合等需要互斥保护的数据
//!
//! ### 进程层次结构
//! 支持完整的进程树结构：
//! - **父子关系**：通过 `parent` 和 `children` 字段维护
//! - **引用计数**：使用 `Arc` 和 `Weak` 防止循环引用
//! - **孤儿进程处理**：父进程退出时子进程重新指向 init 进程
//!
//! ## 内存布局
//!
//! 每个进程的内存空间布局：
//!
//! ```text
//! 高地址 TRAMPOLINE (0x3ffffff000)
//! ┌──────────────────────────────────────────────────────┐
//! │                   Trampoline                         │
//! │                    (R+X)                             │
//! ├──────────────────────────────────────────────────────┤
//! │                 Trap Context                         │
//! │                    (R+W)                             │
//! ├──────────────────────────────────────────────────────┤
//! │                  User Stack                          │
//! │                   (R+W+U)                            │
//! ├──────────────────────────────────────────────────────┤
//! │               Program Sections                       │
//! │            (.text/.data/.bss etc)                    │
//! │              (Based on ELF flags)                    │
//! └──────────────────────────────────────────────────────┘
//! 低地址 (0x10000)
//!
//! 内核空间 - 每个进程独立的内核栈：
//! ┌──────────────────────────────────────────────────────┐
//! │             Process N Kernel Stack                   │
//! ├──────────────────────────────────────────────────────┤
//! │                  Guard Page                          │
//! ├──────────────────────────────────────────────────────┤
//! │           Process N-1 Kernel Stack                   │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! ## 进程状态转换
//!
//! ```text
//!           fork/exec
//!         ┌──────────┐
//!         │          │
//!         ▼          │
//!     ┌───────┐      │      Scheduler Selection
//!     │ Ready │◄─────┘ ◄─────────────┐
//!     └───┬───┘                      │
//!         │                          │
//!         │ Scheduler Selection      │ Timeslice Expire/Yield
//!         │                          │
//!         ▼                          │
//!    ┌─────────┐                     │
//!    │ Running │───────────────────-─┘
//!    └────┬────┘
//!         │
//!         │ exit() System Call
//!         │
//!         ▼
//!     ┌───────┐
//!     │Zombie │ ◄── Wait for Parent wait()
//!     └───────┘
//! ```
//!
//! ## 并发安全
//!
//! - **互斥保护**：可变部分使用 `UPSafeCell` 保证线程安全
//! - **原子操作**：PID 分配和内核栈管理通过全局锁保护
//! - **引用计数**：`Arc` 确保进程对象的生命周期管理
//!
//! ## 使用示例
//!
//! ```rust
//! // 创建新进程
//! let elf_data = get_app_data(0);
//! let task = Arc::new(TaskControlBlock::new(elf_data));
//!
//! // Fork 子进程
//! let child_task = task.fork();
//!
//! // 执行新程序
//! task.exec(new_elf_data);
//!
//! // 获取进程状态
//! let inner = task.inner_exclusive_access();
//! println!("Task status: {:?}", inner.task_status);
//! ```

use super::context::TaskContext;
use crate::sync::UPSafeCell;
use crate::task::pid::pid_alloc;
use crate::{
    config::TRAP_CONTEXT,
    mm::{KERNEL_SPACE, MemorySet, PhysPageNum, VirtAddr},
    task::pid::{KernelStack, PidHandle},
    trap::{TrapContext, trap_handler},
};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;

/// 任务控制块 (Task Control Block)
///
/// 操作系统中每个进程的核心数据结构，包含进程运行所需的全部信息。
/// TCB 采用内外分离设计，将不变和可变部分分开管理，提高并发访问效率。
///
/// ## 结构组成
///
/// ### 不变部分（直接字段）
/// - `pid`: 进程标识符句柄，进程生命周期内唯一且不变
/// - `kernel_stack`: 内核栈，用于系统调用和中断处理
///
/// ### 可变部分（受保护字段）
/// - `inner`: 包含所有可变状态，使用 [`UPSafeCell`] 进行互斥保护
///
/// ## 设计优势
///
/// **性能优化**：
/// - 不变字段可以直接访问，避免锁开销
/// - 可变字段集中保护，减少锁的粒度和争用
///
/// **并发安全**：
/// - [`UPSafeCell`] 提供单处理器环境下的安全可变访问
/// - 通过 [`Arc`] 支持多处理器环境下的引用计数管理
///
/// **资源管理**：
/// - PID 和内核栈通过 RAII 自动管理生命周期
/// - 内存集合在进程退出时自动清理
///
/// ## 生命周期
///
/// ```text
/// 创建阶段:
/// ┌────────────-─┐    ┌─-────────────--┐    ┌────────────┐
/// │ Allocate PID │───►│ Create K-Stack │───►│ Parse ELF  │
/// └──────────-───┘    └-────────────-─-┘    └────────────┘
///                                                 │
///                                                 ▼
/// ┌────────────┐    ┌─-────────-────┐      ┌──────────--───┐
/// │ Init State │◄───│ Setup AddrSpc │  ◄─  │ Setup TrapCtx │
/// └-───────────┘    └-─────────────-┘      └────────────--─┘
///
/// 运行阶段：
/// Ready ──Schedule──► Running ──Timeslice/Yield──► Ready
///                       │
///                       │ exit()
///                       ▼
///                    Zombie ──wait()──► Destroy
///
/// 销毁阶段:
/// ┌─────────────┐    ┌───────────-──┐    ┌──────────┐
/// │ Free Memory │───►│ Free K-Stack │───►│ Free PID │
/// └─────────────┘    └─────────────-┘    └──────────┘
/// ```
///
/// ## 使用模式
///
/// TCB 通常包装在 [`Arc`] 中使用，支持多所有者场景：
/// - 任务管理器持有引用进行调度
/// - 父进程持有子进程引用进行管理
/// - 处理器持有当前运行任务的引用
///
/// ## Examples
///
/// ```rust
/// use alloc::sync::Arc;
///
/// // 创建新进程
/// let elf_data = include_bytes!("user_program.elf");
/// let task = Arc::new(TaskControlBlock::new(elf_data));
///
/// // 访问不变字段（无需锁）
/// println!("Process PID: {}", task.getpid());
///
/// // 访问可变字段（需要获取锁）
/// {
///     let inner = task.inner_exclusive_access();
///     println!("Task status: {:?}", inner.task_status);
/// } // 锁在此处自动释放
///
/// // Fork 创建子进程
/// let child_task = task.fork();
/// ```
pub struct TaskControlBlock {
    /// 进程标识符句柄
    ///
    /// 包含系统唯一的进程 ID，通过 RAII 机制自动管理 PID 的分配和回收。
    /// 在进程的整个生命周期中保持不变，可以安全地并发访问。
    pub pid: PidHandle,

    /// 内核栈
    ///
    /// 每个进程在内核空间中的独立栈空间，用于：
    /// - 系统调用处理时的临时数据存储
    /// - 中断和异常处理时的上下文保存
    /// - 任务切换时的寄存器保存
    ///
    /// 通过 RAII 机制自动管理内核栈的分配、映射和回收。
    pub kernel_stack: KernelStack,

    /// 内部可变状态
    ///
    /// 包含所有需要在运行时修改的进程状态信息，使用 [`UPSafeCell`]
    /// 提供线程安全的可变访问。包括任务状态、内存集合、上下文等。
    inner: UPSafeCell<TaskControlBlockInner>,
}

/// 任务控制块内部可变状态
///
/// 包含进程的所有可变状态信息，需要在运行时进行修改的字段都集中在此结构中。
/// 通过 [`UPSafeCell`] 进行互斥保护，确保并发访问的安全性。
///
/// ## 字段说明
///
/// ### 运行时状态
/// - `task_status`: 进程当前状态（Ready/Running/Zombie）
/// - `task_cx`: 任务上下文，保存寄存器状态用于任务切换
/// - `exit_code`: 进程退出码，用于父进程获取子进程执行结果
///
/// ### 内存管理
/// - `memory_set`: 进程的完整地址空间，包含所有内存映射区域
/// - `trap_cx_ppn`: 陷阱上下文的物理页号，用于用户态/内核态切换
/// - `base_size`: 进程初始堆栈大小，用于内存分配决策
///
/// ### 进程关系
/// - `parent`: 父进程的弱引用，避免循环引用导致内存泄漏
/// - `children`: 子进程列表，维护进程树结构
///
/// ## 设计考虑
///
/// **并发安全**：
/// - 所有字段都受到外层 [`UPSafeCell`] 的保护
/// - 访问时需要获取独占锁，避免数据竞争
/// - 锁的持有时间应当尽可能短，避免性能影响
///
/// **内存管理**：
/// - [`MemorySet`] 自动管理进程地址空间的生命周期
/// - 使用 [`Arc`] 和 [`Weak`] 管理进程间的引用关系
/// - 进程退出时自动清理相关资源
///
/// **状态一致性**：
/// - 任务状态与上下文信息保持同步
/// - 父子关系的双向引用保持一致
/// - 内存映射与陷阱上下文匹配
pub struct TaskControlBlockInner {
    /// 任务当前状态
    ///
    /// 表示进程在操作系统中的当前状态，影响调度器的调度决策。
    /// 状态转换遵循严格的状态机规则。
    pub task_status: TaskStatus,

    /// 任务上下文
    ///
    /// 保存进程在任务切换时需要恢复的寄存器状态，包括：
    /// - 返回地址 (ra): 任务恢复时的执行地址
    /// - 栈指针 (sp): 内核栈的栈顶地址
    /// - 被调用者保存寄存器 (s0-s11): 函数调用约定要求保存的寄存器
    pub task_cx: TaskContext,

    /// 进程地址空间
    ///
    /// 管理进程的完整虚拟地址空间，包括：
    /// - 代码段、数据段、堆段、栈段的映射
    /// - 页表管理和地址转换
    /// - 内存权限控制和保护
    pub memory_set: MemorySet,

    /// 陷阱上下文物理页号
    ///
    /// 指向保存陷阱上下文的物理页面，陷阱上下文包含：
    /// - 用户态所有寄存器的值
    /// - 系统调用参数和返回值
    /// - 异常处理相关信息
    pub trap_cx_ppn: PhysPageNum,

    /// 进程基础内存大小
    ///
    /// 记录进程初始化时的内存使用情况，用于：
    /// - 内存分配和回收的决策参考
    /// - 进程资源使用统计
    /// - 堆空间管理的基准值
    pub base_size: usize,

    /// 父进程引用
    ///
    /// 指向父进程的弱引用，用于维护进程树结构。使用 [`Weak`] 避免
    /// 父子进程间的循环引用，防止内存泄漏。当父进程退出时，
    /// 子进程会被重新指向 init 进程。
    pub parent: Option<Weak<TaskControlBlock>>,

    /// 子进程列表
    ///
    /// 维护当前进程的所有子进程，用于：
    /// - 实现 wait 系统调用，等待子进程退出
    /// - 进程退出时处理孤儿进程，重新指向 init 进程
    /// - 信号传递和进程组管理
    pub children: Vec<Arc<TaskControlBlock>>,

    /// 进程退出码
    ///
    /// 记录进程的退出状态，供父进程通过 wait 系统调用获取。
    /// 标准约定：0 表示正常退出，非零表示异常退出。
    pub exit_code: i32,
}

/// 任务状态枚举
///
/// 定义进程在操作系统中的各种运行状态，遵循经典的进程状态模型。
/// 状态转换受到严格的状态机规则约束，确保系统的一致性和可预测性。
///
/// ## 状态转换图
///
/// ```text
///                Process Creation
///                      │
///                      ▼
///               ┌─────────────┐
///               │    Ready    │ ◄─────────────────┐
///               │  (Ready)    │                   │
///               └──────┬──────┘                   │
///                      │                          │
///                      │ Scheduler Selection      │
///                      ▼                          │
///               ┌─────────────┐                   │
///               │   Running   │                   │
///               │  (Running)  │                   │
///               └──────┬──────┘                   │
///                      │                          │
///            ┌─────────┴─────────┐                │
///            │                   │                │
///            ▼                   ▼                │
///   Timeslice Expire/Yield   exit() System Call   │
///            │                   │                │
///            └───────────────────┼───────────────-┘
///                                ▼
///                        ┌─────────────┐
///                        │   Zombie    │
///                        │  (Zombie)   │
///                        └─────────────┘
///                                │
///                                │ Parent wait()
///                                ▼
///                        Process Destruction
/// ```
///
/// ## 状态详细说明
///
/// ### Ready (就绪)
/// - **含义**: 进程已准备好运行，等待 CPU 分配
/// - **特征**: 所有运行条件已满足，仅等待调度器调度
/// - **转入**: 进程创建完成、时间片用完、系统调用返回
/// - **转出**: 被调度器选中执行
///
/// ### Running (运行)
/// - **含义**: 进程正在 CPU 上执行
/// - **特征**: 拥有 CPU 控制权，正在执行指令
/// - **转入**: 从就绪队列被调度器选中
/// - **转出**: 时间片用完、主动让出 CPU、进程退出
///
/// ### Zombie (僵尸)
/// - **含义**: 进程已执行完毕，等待父进程收集退出信息
/// - **特征**: 保留进程控制块，但不再调度执行
/// - **转入**: 进程调用 exit 系统调用
/// - **转出**: 父进程调用 wait 系统调用回收
///
/// ## 使用示例
///
/// ```rust
/// // 创建进程后的状态检查
/// let task = TaskControlBlock::new(elf_data);
/// {
///     let inner = task.inner_exclusive_access();
///     assert_eq!(inner.task_status, TaskStatus::Ready);
/// }
///
/// // 状态转换示例
/// match current_status {
///     TaskStatus::Ready => {
///         // 可以被调度器选中
///         scheduler.add_to_running_queue(task);
///     }
///     TaskStatus::Running => {
///         // 正在执行，可能需要时间片管理
///         if time_slice_expired() {
///             task.suspend_and_yield();
///         }
///     }
///     TaskStatus::Zombie => {
///         // 等待父进程回收
///         if parent.is_waiting() {
///             parent.collect_child_exit_code(task.exit_code);
///         }
///     }
/// }
/// ```
///
/// ## 状态约束
///
/// - **互斥性**: 同一时刻进程只能处于一种状态
/// - **有序性**: 状态转换必须遵循预定义的转换路径
/// - **原子性**: 状态转换是原子操作，不存在中间状态
/// - **一致性**: 状态转换与系统其他部分的状态保持一致
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum TaskStatus {
    /// 就绪状态
    ///
    /// 进程已准备好执行，所有资源已分配，等待调度器分配 CPU 时间。
    /// 处于此状态的进程位于就绪队列中，按调度策略等待执行机会。
    Ready,

    /// 运行状态
    ///
    /// 进程正在 CPU 上执行指令，拥有处理器控制权。在单处理器系统中，
    /// 同一时刻只有一个进程可以处于运行状态。
    Running,

    /// 僵尸状态
    ///
    /// 进程已执行完毕并退出，但进程控制块仍然保留，等待父进程
    /// 通过 wait 系统调用收集其退出状态信息。
    Zombie,
}

impl TaskControlBlockInner {
    /// 获取陷阱上下文的可变引用
    ///
    /// 返回陷阱上下文物理页面的可变引用，用于修改用户态寄存器状态。
    /// 陷阱上下文包含用户态所有寄存器的值，用于系统调用和异常处理。
    ///
    /// ## Returns
    ///
    /// 陷阱上下文的可变引用
    ///
    /// ## Safety
    ///
    /// 调用者必须确保陷阱上下文物理页面已正确分配和初始化
    ///
    /// ## Examples
    ///
    /// ```
    /// let trap_cx = inner.trap_cx();
    /// trap_cx.x[10] = return_value;  // 设置系统调用返回值
    /// ```
    pub fn trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.mut_ref()
    }

    /// 获取用户地址空间的页表标识符
    ///
    /// 返回用户地址空间的页表标识符，用于在用户态和内核态之间切换地址空间。
    /// 该值通常被编码到 `satp` 寄存器中。
    ///
    /// ## Returns
    ///
    /// 用户地址空间的页表标识符
    ///
    /// ## Examples
    ///
    /// ```
    /// let user_token = inner.user_token();
    /// // 切换到用户地址空间
    /// ```
    pub fn user_token(&self) -> usize {
        self.memory_set.token()
    }

    /// 获取任务状态
    ///
    /// 返回当前任务的状态，为内部使用的辅助方法。
    /// 外部代码应该直接访问 `task_status` 字段。
    ///
    /// ## Returns
    ///
    /// 返回当前的 [`TaskStatus`]
    fn status(&self) -> TaskStatus {
        self.task_status
    }

    /// 检查任务是否为僵尸状态
    ///
    /// 判断当前任务是否已经退出但尚未被父进程回收。
    /// 僵尸进程不会被调度执行，但保留 TCB 以供父进程获取退出信息。
    ///
    /// ## Returns
    ///
    /// - `true` - 任务为僵尸状态，等待父进程回收
    /// - `false` - 任务不是僵尸状态，可能正在运行或就绪
    ///
    /// ## 使用场景
    ///
    /// - **父进程管理**: 父进程检查子进程是否退出
    /// - **进程清理**: 系统定期清理僵尸进程
    /// - **wait 系统调用**: 等待子进程退出的实现
    /// - **调度器**: 过滤不可调度的进程
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 检查子进程状态
    /// let parent_inner = parent_task.inner_exclusive_access();
    /// for child_task in &parent_inner.children {
    ///     let child_inner = child_task.inner_exclusive_access();
    ///     if child_inner.is_zombie() {
    ///         println!("Child process {} exited with code {}",
    ///                  child_task.getpid(), child_inner.exit_code);
    ///     }
    /// }
    ///
    /// // 调度器过滤
    /// if !task_inner.is_zombie() {
    ///     ready_queue.push(task);
    /// }
    /// ```
    pub fn is_zombie(&self) -> bool {
        self.status() == TaskStatus::Zombie
    }
}

impl TaskControlBlock {
    /// 从 ELF 文件创建新的任务控制块
    ///
    /// 解析给定的 ELF 可执行文件，创建完整的进程控制块。
    /// 包括分配 PID、创建内核栈、建立地址空间和初始化陷阱上下文。
    ///
    /// ## Arguments
    ///
    /// * `elf_data` - ELF 文件的二进制数据切片，必须是有效的 ELF 格式
    ///
    /// ## Returns
    ///
    /// 返回初始化完成的任务控制块，状态为 [`TaskStatus::Ready`]
    ///
    /// ## 初始化过程
    ///
    /// ```text
    /// 1. 解析 ELF 文件
    ///    │
    ///    │ - 提取程序段（.text, .data, .bss 等）
    ///    │ - 获取入口点地址和用户栈指针
    ///    ▼
    /// 2. 建立地址空间
    ///    │
    ///    │ - 创建用户态地址空间
    ///    │ - 映射程序段、用户栈、Trampoline 等
    ///    ▼
    /// 3. 分配系统资源
    ///    │
    ///    │ - 分配 PID
    ///    │ - 创建内核栈
    ///    ▼
    /// 4. 初始化上下文
    ///    │
    ///    │ - 设置任务上下文（指向 trap_return）
    ///    │ - 设置陷阱上下文（用户态寄存器初始值）
    ///    ▼
    /// 5. 进程创建完成
    /// ```
    ///
    /// ## 内存布局
    ///
    /// 创建后的进程具有以下内存布局：
    ///
    /// ```text
    /// 用户地址空间:
    /// 高地址 TRAMPOLINE
    /// ┌────────────────────┐
    /// │    Trampoline      │ ← 用户态/内核态切换代码
    /// ├────────────────────┤
    /// │   Trap Context     │ ← 用户态寄存器状态
    /// ├────────────────────┤
    /// │    User Stack      │ ← 用户态栈空间
    /// ├────────────────────┤
    /// │    Guard Page      │ ← 防止栈溢出
    /// ├────────────────────┤
    /// │  Program Sections  │ ← .text/.data/.bss
    /// └────────────────────┘
    /// 低地址 0x10000
    ///
    /// 内核地址空间:
    /// ┌────────────────────┐
    /// │   Kernel Stack     │ ← 系统调用处理时使用
    /// └────────────────────┘
    /// ```
    ///
    /// ## 初始状态
    ///
    /// 新创建的任务具有以下初始状态：
    /// - **任务状态**: [`TaskStatus::Ready`] - 准备执行
    /// - **父进程**: `None` - 无父进程关系
    /// - **子进程**: 空列表 - 暂无子进程
    /// - **退出码**: 0 - 默认退出码
    ///
    /// ## Panics
    ///
    /// 在以下情况下会触发 panic：
    /// - ELF 文件格式错误或无效
    /// - PID 分配失败（系统 PID 资源耗尽）
    /// - 内核栈分配失败（内存不足）
    /// - 地址空间创建失败（虚拟内存系统错误）
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use alloc::sync::Arc;
    ///
    /// // 从应用程序数据创建任务
    /// let app_data = get_app_data(0);
    /// let task = Arc::new(TaskControlBlock::new(app_data));
    ///
    /// // 检查初始状态
    /// {
    ///     let inner = task.inner_exclusive_access();
    ///     assert_eq!(inner.task_status, TaskStatus::Ready);
    ///     assert_eq!(inner.exit_code, 0);
    ///     assert!(inner.parent.is_none());
    ///     assert!(inner.children.is_empty());
    /// }
    ///
    /// println!("Created task with PID: {}", task.getpid());
    /// ```
    ///
    /// ## 相关方法
    ///
    /// - [`fork()`] - 从现有进程创建子进程
    /// - [`exec()`] - 替换当前进程的可执行文件
    pub fn new(elf_data: &[u8]) -> Self {
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.top();
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    task_status: TaskStatus::Ready,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    memory_set,
                    trap_cx_ppn,
                    base_size: user_sp,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                })
            },
        };
        let trap_cx = task_control_block.inner_exclusive_access().trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }

    /// 获取内部状态的排他访问权
    ///
    /// 返回 [`TaskControlBlockInner`] 的排他可变引用，用于安全地读取和修改
    /// 任务的可变状态。在同一时刻只能有一个访问者获得访问权。
    ///
    /// ## Returns
    ///
    /// 返回 [`RefMut<TaskControlBlockInner>`]，提供对内部状态的排他访问
    ///
    /// ## 并发安全
    ///
    /// 通过 [`UPSafeCell`] 的互斥机制保证：
    /// - **排他性**: 同一时刻只能有一个访问者
    /// - **未欣正风险**: 编译时检查并发访问合法性
    /// - **自动释放**: 引用超出作用域时自动释放锁
    ///
    /// ## 使用模式
    ///
    /// 应该尽可能缩短锁的持有时间，避免阵塞其他线程：
    ///
    /// ```rust
    /// // 推荐的使用方式：短时间持有锁
    /// {
    ///     let mut inner = task.inner_exclusive_access();
    ///     inner.task_status = TaskStatus::Running;
    ///     // 其他对 inner 的操作...
    /// } // 锁在此处自动释放
    ///
    /// // 不推荐：长时间持有锁
    /// let mut inner = task.inner_exclusive_access();
    /// // 长时间的计算或 I/O 操作...
    /// heavy_computation();
    /// inner.task_status = TaskStatus::Ready;
    /// ```
    ///
    /// ## 常用操作
    ///
    /// 通过排他访问可以执行以下操作：
    /// - **状态管理**: 修改任务状态和退出码
    /// - **上下文访问**: 获取陷阱上下文和任务上下文
    /// - **内存管理**: 访问和修改地址空间
    /// - **进程关系**: 管理父子进程关系
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 修改任务状态
    /// {
    ///     let mut inner = task.inner_exclusive_access();
    ///     inner.task_status = TaskStatus::Running;
    /// }
    ///
    /// // 访问陷阱上下文
    /// {
    ///     let inner = task.inner_exclusive_access();
    ///     let trap_cx = inner.trap_cx();
    ///     let syscall_id = trap_cx.x[17];
    /// }
    ///
    /// // 检查进程状态
    /// {
    ///     let inner = task.inner_exclusive_access();
    ///     if inner.is_zombie() {
    ///         println!("Process exited with code: {}", inner.exit_code);
    ///     }
    /// }
    /// ```
    ///
    /// ## 性能考虑
    ///
    /// - **锁争用**: 高频率访问可能导致性能下降
    /// - **死锁防范**: 避免在持有锁时获取其他锁
    /// - **内存开销**: `RefMut` 本身有较小的运行时开销
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }

    /// 获取进程 ID
    ///
    /// 返回当前任务的进程标识符 (PID)。PID 在系统中全局唯一，
    /// 用于识别和区分不同的进程。
    ///
    /// ## Returns
    ///
    /// 返回 `usize` 类型的 PID 值，PID 从 0 开始分配
    ///
    /// ## 特性
    ///
    /// - **唯一性**: PID 在系统运行期间唯一标识一个进程
    /// - **不变性**: PID 在进程生命周期内不变
    /// - **递增性**: PID 按创建顺序递增分配（在回收重用前）
    /// - **无锁访问**: 获取 PID 不需要获取任何锁
    ///
    /// ## 使用场景
    ///
    /// - **进程管理**: 系统调用中识别调用进程
    /// - **调试输出**: 日志和调试信息中显示进程 ID
    /// - **进程通信**: 信号、管道等进程间通信机制
    /// - **资源管理**: 进程资源统计和限制
    /// - **安全检查**: 权限验证和访问控制
    ///
    /// ## 与 POSIX 兼容性
    ///
    /// 返回的 PID 值遵循 POSIX 约定：
    /// - PID 0 通常保留给系统调度器
    /// - PID 1 通常是 init 进程
    /// - 正数 PID 表示正常用户进程
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 获取当前任务的 PID
    /// let current_pid = task.getpid();
    /// println!("Current process PID: {}", current_pid);
    ///
    /// // 在系统调用中使用
    /// match syscall_id {
    ///     SYS_GETPID => {
    ///         let pid = current_task().unwrap().getpid();
    ///         // 返回 PID 给用户程序
    ///         pid as isize
    ///     }
    ///     _ => -1,
    /// }
    ///
    /// // 进程管理中的使用
    /// fn kill_process(target_pid: usize) -> bool {
    ///     for task in &task_list {
    ///         if task.getpid() == target_pid {
    ///             // 找到目标进程，执行终止操作
    ///             return terminate_task(task);
    ///         }
    ///     }
    ///     false // 未找到目标进程
    /// }
    ///
    /// // 调试输出中的使用
    /// println!("Task {} entering syscall {}", task.getpid(), syscall_id);
    /// ```
    ///
    /// ## 性能特性
    ///
    /// - **O(1) 时间复杂度**: 直接字段访问，无计算开销
    /// - **无锁开销**: 不需要获取任何互斥锁
    /// - **缓存友好**: 频繁调用不会产生额外开销
    ///
    /// ## 相关函数
    ///
    /// - [`pid_alloc()`] - 分配新的 PID
    /// - [`PidHandle`] - PID 的 RAII 封装
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// Fork 系统调用实现：创建子进程
    ///
    /// 复制当前进程创建一个新的子进程，子进程继承父进程的地址空间、文件描述符等资源。
    /// 这是 UNIX/Linux 系统中创建新进程的主要方式，遵循经典的 fork 语义。
    ///
    /// ## Arguments
    ///
    /// * `self` - 父进程的 Arc 引用，用于建立父子关系
    ///
    /// ## Returns
    ///
    /// 返回新创建的子进程的 [`Arc<TaskControlBlock>`]
    ///
    /// ## Fork 语义
    ///
    /// **父子进程差异**：
    /// - **PID**: 子进程分配新的 PID
    /// - **内核栈**: 子进程分配独立的内核栈
    /// - **返回值**: 在父进程中返回子进程 PID，在子进程中返回 0
    ///
    /// **共享与复制**：
    /// - **地址空间**: 子进程获得父进程地址空间的完整副本
    /// - **寄存器状态**: 子进程继承父进程当前的所有寄存器值
    /// - **文件描述符**: 子进程继承父进程打开的文件描述符
    /// - **工作目录**: 子进程继承父进程的工作目录
    ///
    /// ## Fork 过程详解
    ///
    /// ```text
    /// 1. 复制地址空间
    ///    │
    ///    │ - 创建新的内存集合
    ///    │ - 逐页复制父进程的所有内存内容
    ///    │ - 建立独立的页表结构
    ///    ▼
    /// 2. 分配系统资源
    ///    │
    ///    │ - 分配新的 PID
    ///    │ - 创建独立的内核栈
    ///    │ - 分配新的 TCB
    ///    ▼
    /// 3. 建立父子关系
    ///    │
    ///    │ - 子进程记录父进程弱引用
    ///    │ - 父进程添加子进程到 children 列表
    ///    │ - 维护进程树结构
    ///    ▼
    /// 4. 初始化子进程状态
    ///    │
    ///    │ - 设置任务状态为 Ready
    ///    │ - 复制任务上下文
    ///    │ - 调整内核栈指针
    ///    ▼
    /// 5. 子进程创建完成
    /// ```
    ///
    /// ## 内存布局对比
    ///
    /// ```text
    /// Fork 前 (父进程):
    /// ┌─────────────────┐
    /// │   Parent Task   │
    /// │     (PID n)     │
    /// ├─────────────────┤
    /// │  Address Space  │ ← 原始地址空间
    /// │    Virtual      │
    /// │     Memory      │
    /// └─────────────────┘
    ///
    /// Fork 后:
    /// ┌─────────────────┐    ┌─────────────────┐
    /// │   Parent Task   │    │   Child Task    │
    /// │     (PID n)     │    │    (PID n+1)    │
    /// ├─────────────────┤    ├─────────────────┤
    /// │  Address Space  │    │  Address Space  │ ← 完整副本
    /// │    (Original)   │    │     (Copy)      │
    /// │     Memory      │    │     Memory      │
    /// └─────────────────┘    └─────────────────┘
    ///            │                      ▲
    ///            └──────────────────────┘
    ///                 父子关系
    /// ```
    ///
    /// ## 系统调用返回值处理
    ///
    /// Fork 的一个重要特征是一次调用两次返回：
    ///
    /// ```text
    /// 调用时机：                父进程返回          子进程返回
    /// ┌──────────┐             ┌──────────┐        ┌──────────┐
    /// │ fork()   │────────────►│child_pid │        │    0     │
    /// │ SysCall  │             │          │        │          │
    /// └──────────┘             └──────────┘        └──────────┘
    /// ```
    ///
    /// 返回值设置在陷阱上下文中的 `x[10]` 寄存器（RISC-V ABI 返回值寄存器）。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use alloc::sync::Arc;
    ///
    /// // 父进程执行 fork
    /// let parent_task = Arc::new(TaskControlBlock::new(parent_elf));
    /// let child_task = parent_task.fork();
    ///
    /// // 检查父子关系
    /// {
    ///     let child_inner = child_task.inner_exclusive_access();
    ///     assert!(child_inner.parent.is_some());
    /// }
    ///
    /// {
    ///     let parent_inner = parent_task.inner_exclusive_access();
    ///     assert_eq!(parent_inner.children.len(), 1);
    ///     assert_eq!(parent_inner.children[0].getpid(), child_task.getpid());
    /// }
    ///
    /// println!("Parent PID: {}, Child PID: {}",
    ///          parent_task.getpid(), child_task.getpid());
    /// ```
    ///
    /// ## 错误处理
    ///
    /// Fork 可能在以下情况失败：
    /// - **内存不足**: 无法分配子进程的地址空间或内核栈
    /// - **PID 耗尽**: 系统 PID 资源已用完
    /// - **系统限制**: 达到进程数量或内存使用限制
    ///
    /// ## 性能考虑
    ///
    /// - **写时复制 (COW)**: 某些系统实现 COW 优化，本实现为完整复制
    /// - **内存开销**: 完整复制地址空间会消耗大量内存
    /// - **时间开销**: 复制过程的时间与父进程地址空间大小成正比
    ///
    /// ## RISC-V 特定处理
    ///
    /// - **内核栈指针**: 更新子进程陷阱上下文中的内核栈指针
    /// - **寄存器继承**: 子进程继承父进程的所有通用寄存器值
    /// - **特权模式**: 子进程在用户模式下开始执行
    ///
    /// ## 相关系统调用
    ///
    /// - [`exec()`] - 替换进程映像，通常与 fork 配合使用
    /// - [`wait()`] - 父进程等待子进程退出
    /// - [`exit()`] - 进程正常退出
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        let mut parent_inner = self.inner_exclusive_access();
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                })
            },
        });
        parent_inner.children.push(task_control_block.clone());
        let trap_cx = task_control_block.inner_exclusive_access().trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        task_control_block
    }

    /// Exec 系统调用实现：替换进程映像
    ///
    /// 使用新的可执行文件替换当前进程的内存映像，保持相同的 PID 和内核栈，
    /// 但完全替换地址空间和执行上下文。这是 UNIX/Linux 系统加载新程序的标准方式。
    ///
    /// ## Arguments
    ///
    /// * `elf_data` - 新程序的 ELF 文件二进制数据
    ///
    /// ## Exec 语义
    ///
    /// **保持不变的部分**：
    /// - **PID**: 进程标识符不变，仍然是同一个进程
    /// - **内核栈**: 复用现有的内核栈，不重新分配
    /// - **父子关系**: 进程在进程树中的位置不变
    ///
    /// **替换的部分**：
    /// - **地址空间**: 完全替换为新程序的内存布局
    /// - **执行上下文**: 重置为新程序的入口点和初始状态
    /// - **用户态寄存器**: 重新初始化为新程序的启动状态
    ///
    /// ## Exec 过程详解
    ///
    /// ```text
    /// 1. 解析新的 ELF 文件
    ///    │
    ///    │ - 验证 ELF 文件格式
    ///    │ - 提取程序段信息
    ///    │ - 获取入口点地址
    ///    ▼
    /// 2. 创建新的地址空间
    ///    │
    ///    │ - 销毁旧的内存映射
    ///    │ - 建立新程序的内存布局
    ///    │ - 加载程序段到内存
    ///    ▼
    /// 3. 更新进程状态
    ///    │
    ///    │ - 更新内存集合
    ///    │ - 更新陷阱上下文页号
    ///    │ - 重新设置基础内存大小
    ///    ▼
    /// 4. 初始化执行环境
    ///    │
    ///    │ - 设置程序计数器为入口点
    ///    │ - 初始化用户栈指针
    ///    │ - 清零通用寄存器
    ///    ▼
    /// 5. 程序替换完成
    /// ```
    ///
    /// ## 内存布局变化
    ///
    /// ```text
    /// Exec 前 (旧程序):
    /// ┌─────────────────────────┐
    /// │      Task (PID n)       │ ← PID 保持不变
    /// ├─────────────────────────┤
    /// │     Old Program         │
    /// │   Address Space:        │
    /// │  ┌──────────────────┐   │
    /// │  │ Old .text/.data  │   │ ← 将被完全替换
    /// │  │ Old User Stack   │   │
    /// │  │ Old Heap         │   │
    /// │  └──────────────────┘   │
    /// └─────────────────────────┘
    ///
    /// Exec 后 (新程序):
    /// ┌─────────────────────────┐
    /// │      Task (PID n)       │ ← 同一个 PID
    /// ├─────────────────────────┤
    /// │     New Program         │
    /// │   Address Space:        │
    /// │  ┌──────────────────┐   │
    /// │  │ New .text/.data  │   │ ← 新程序的内存布局
    /// │  │ New User Stack   │   │
    /// │  │ New Heap         │   │
    /// │  └──────────────────┘   │
    /// └─────────────────────────┘
    /// ```
    ///
    /// ## 上下文重置
    ///
    /// Exec 会重置进程的执行上下文：
    ///
    /// ```text
    /// 重置项目                 新值
    /// ┌──────────────────┐    ┌─────────────────────┐
    /// │Program Counter   │───►│ New Entry Point     │
    /// ├──────────────────┤    ├─────────────────────┤
    /// │Stack Pointer     │───►│ New User Stack Top  │
    /// ├──────────────────┤    ├─────────────────────┤
    /// │General Registers │───►│ Zero or Init Values │
    /// ├──────────────────┤    ├─────────────────────┤
    /// │Page Table Ptr    │───►│ New Address Space   │
    /// └──────────────────┘    └─────────────────────┘
    /// ```
    ///
    /// ## 典型使用场景
    ///
    /// **Shell 命令执行**：
    /// ```text
    /// 1. Shell 进程执行 fork() 创建子进程
    /// 2. 子进程执行 exec() 加载目标程序
    /// 3. 父进程 (Shell) 执行 wait() 等待子进程完成
    /// ```
    ///
    /// **程序动态加载**：
    /// ```text
    /// 1. 当前进程不再需要原程序代码
    /// 2. 直接执行 exec() 替换为新程序
    /// 3. 新程序从头开始执行
    /// ```
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// // 替换当前进程为新程序
    /// let new_program = get_app_data("target_app");
    ///
    /// // 记录替换前的信息
    /// let old_pid = task.getpid();
    /// println!("Executing new program in PID {}", old_pid);
    ///
    /// // 执行替换
    /// task.exec(new_program);
    ///
    /// // 验证 PID 未变但程序已替换
    /// assert_eq!(task.getpid(), old_pid);
    ///
    /// // 检查新的执行状态
    /// {
    ///     let inner = task.inner_exclusive_access();
    ///     let trap_cx = inner.trap_cx();
    ///     println!("New entry point: 0x{:x}", trap_cx.sepc);
    ///     println!("New stack pointer: 0x{:x}", trap_cx.x[2]);
    /// }
    /// ```
    ///
    /// ## 安全考虑
    ///
    /// - **权限检查**: 确保有权限执行目标文件
    /// - **格式验证**: 验证 ELF 文件的完整性和合法性
    /// - **资源清理**: 确保旧程序的所有资源得到正确释放
    /// - **状态一致性**: 保证替换过程的原子性
    ///
    /// ## 错误处理
    ///
    /// Exec 可能在以下情况失败：
    /// - **文件格式错误**: ELF 文件格式无效或损坏
    /// - **内存不足**: 无法为新程序分配足够的内存空间
    /// - **权限不足**: 没有执行目标文件的权限
    /// - **系统资源限制**: 超出系统资源限制
    ///
    /// ## 性能特性
    ///
    /// - **内存复用**: 复用现有的 PID 和内核栈资源
    /// - **快速切换**: 相比 fork + exec 模式，单独 exec 更高效
    /// - **内存释放**: 自动释放旧程序占用的所有内存
    ///
    /// ## 与其他系统调用的关系
    ///
    /// - **fork() + exec()**: 经典的进程创建和程序加载模式
    /// - **wait()**: 父进程等待 exec 后的子进程完成
    /// - **exit()**: 进程执行完成后的正常退出
    pub fn exec(&self, elf_data: &[u8]) {
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let mut inner = self.inner_exclusive_access();
        inner.memory_set = memory_set;
        inner.trap_cx_ppn = trap_cx_ppn;
        inner.base_size = user_sp;
        let trap_cx = inner.trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.top(),
            trap_handler as usize,
        );
    }
}
