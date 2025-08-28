//! # 进程管理器模块
//!
//! 提供进程调度和管理的核心功能，实现就绪队列的维护和进程分发。
//! 采用多级反馈队列 (MLFQ) 调度策略，实现可降低优先级的抢占式调度。
//!
//! ## 核心组件
//!
//! - [`ProcessManager`] - 进程管理器，维护就绪进程队列
//! - [`PROCESS_MANAGER`] - 全局进程管理器实例
//! - [`add_process`] - 向就绪队列添加进程的全局接口
//! - [`fetch_process`] - 从就绪队列获取进程的全局接口
//!
//! ## 设计原理
//!
//! ### 调度策略
//!
//! 采用 **多级反馈队列 (MLFQ)** 调度算法：
//! - **优先级分级**：维护多个优先级不同的就绪队列
//! - **动态降级**：进程用完时间片后降级到低优先级队列
//! - **响应性优化**：新进程和I/O密集型进程优先调度
//! - **公平性保证**：避免长期饥饿，底层队列也会被调度
//!
//! ### 数据结构选择
//!
//! 使用 `Vec<VecDeque<Arc<ProcessControlBlock>>>` 作为多级就绪队列：
//! - **多级队列**：每个优先级都有独立的双端队列
//! - **优先级调度**：总是从最高优先级非空队列取进程
//! - **动态时间片**：不同优先级队列使用不同的时间片长度
//! - **时间复杂度**：入队和出队操作均为 O(1)
//!
//! ### 并发安全
//!
//! 通过 `UPSafeCell` 实现单处理器环境下的线程安全：
//! - **互斥访问**：确保同一时间只有一个线程可以修改就绪队列
//! - **内部可变性**：允许在不可变引用下修改内部数据
//! - **无锁设计**：在单核环境下避免昂贵的锁操作
//!
//! ## 调度流程
//!
//! ```text
//! Process Creation/Wake-up    Process Scheduling
//!         │                           │
//!         ▼                           ▼
//! ┌──────────────┐             ┌──────────────┐
//! │  add_process │             │ fetch_process│
//! │  (Enqueue)   │             │  (Dequeue)   │
//! └──────┬───────┘             └──────┬───────┘
//!        │                            │
//!        ▼                            ▼
//! ┌───────────────────────────────────────-----──----─┐
//! │            Ready Queue                            │
//! │  ┌───---──┐  ┌────---─┐  ┌──---───┐  ┌─────---┐   │
//! │  │Process │◄─│Process │◄─│Process │◄─│Process │   │
//! │  │    A   │  │    B   │  │    C   │  │    D   │   │
//! │  └────---─┘  └---─────┘  └─────---┘  └──---───┘   │
//! │    ▲                                       │      │
//! │    │ Add New Process       Get Next Process│      │
//! │    └──────────────────────------------──-──┘      │
//! └────────────────────────────────---─────────------─┘
//! ```
//!
//! ## 性能特征
//!
//! - **入队延迟**: O(1) - 常数时间复杂度
//! - **出队延迟**: O(1) - 常数时间复杂度  
//! - **内存开销**: 每个进程一个 `Arc` 指针 (8 bytes)
//! - **缓存友好性**: 连续内存布局提供良好的缓存局部性
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::process::manager::{add_process, fetch_process};
//! use crate::process::process::ProcessControlBlock;
//! use alloc::sync::Arc;
//!
//! // 添加新进程到就绪队列
//! let new_process = Arc::new(ProcessControlBlock::new(app_data));
//! add_process(new_process);
//!
//! // 调度器获取下一个进程
//! if let Some(process) = fetch_process() {
//!     println!("调度进程 PID: {}", process.getpid());
//!     // 执行进程...
//! } else {
//!     println!("没有就绪进程，进入空闲状态");
//! }
//! ```

use crate::process::process::ProcessControlBlock;
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use alloc::{
    collections::{BTreeMap, vec_deque::VecDeque},
    sync::Arc,
};
use lazy_static::lazy_static;

/// 进程管理器
///
/// 负责维护系统中所有就绪态进程的队列，提供高效的进程调度服务。
/// 采用 FIFO 调度策略，确保进程调度的公平性和可预测性。
///
/// ## 设计特点
///
/// ### 数据结构
/// - **就绪队列**: 使用 `VecDeque` 实现双端队列，支持 O(1) 的队首删除和队尾插入
/// - **进程引用**: 存储 `Arc<ProcessControlBlock>` 而非进程本身，支持共享所有权
/// - **内存布局**: 连续内存分配提供良好的缓存局部性
///
/// ### 调度算法
/// ```text
/// Schedule Order: Process1 → Process2 → Process3 → Process4
///          ┌──---───┐  ┌────---─┐  ┌─---────┐  ┌───---──┐
/// Enqueue ►│Process4│◄─│Process3│◄─│Process2│◄─│Process1│◄─── Dequeue
///          └─────---┘  └───---──┘  └────---─┘  └---─────┘
///         (最新)                      (最老)
/// ```
///
/// ### 线程安全
/// 虽然 `ProcessManager` 本身不提供线程安全保证，但通过全局的 `UPSafeCell`
/// 包装器确保在单处理器环境下的并发安全访问。
///
/// ## 使用模式
///
/// 通常不直接实例化 `ProcessManager`，而是通过全局函数接口使用：
/// ```rust
/// // 推荐方式：通过全局接口使用
/// add_process(new_process);
/// let next_process = fetch_process();
///
/// // 不推荐：直接操作实例
/// let mut manager = ProcessManager::new();
/// manager.add(process);
/// ```
///
/// ## 性能保证
///
/// - **入队延迟**: O(1) 常数时间
/// - **出队延迟**: O(1) 常数时间
/// - **内存开销**: 8 bytes per process (仅存储 Arc 指针)
/// - **扩容策略**: 按需自动扩容，避免频繁内存分配
pub struct ProcessManager {
    /// 多级就绪进程队列
    ///
    /// 使用多个双端队列实现不同优先级的进程调度。
    /// 索引 0 为最高优先级队列，索引越大优先级越低。
    /// 调度器总是从最高优先级非空队列取进程执行。
    ///
    /// ## 队列特性
    /// - **优先级调度**: 高优先级队列优先被调度
    /// - **动态降级**: 用完时间片的进程降级到下一队列
    /// - **时间片分配**: 不同队列有不同的时间片长度
    /// - **高效操作**: 每个队列的入队和出队均为 O(1)
    ready_queues: Vec<VecDeque<Arc<ProcessControlBlock>>>,

    /// 每个队列的时间片长度（时钟周期数）
    ///
    /// 优先级越高的队列时间片越短，保证响应性；
    /// 优先级越低的队列时间片越长，提高吞吐量。
    time_slices: Vec<usize>,

    /// 队列数量
    queue_count: usize,
}

impl ProcessManager {
    /// 创建新的进程管理器实例
    ///
    /// 初始化一个空的就绪队列，准备接收和调度进程。
    /// 新创建的管理器不包含任何进程，队列容量为 0。
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `ProcessManager` 实例，包含：
    /// - 空的就绪队列（容量为 0）
    /// - 所有计数器重置为 0
    ///
    /// ## 性能特征
    ///
    /// - **时间复杂度**: O(1) - 常数时间初始化
    /// - **内存分配**: 不进行堆内存分配
    /// - **初始容量**: 0，按需扩容以优化内存使用
    ///
    /// ## 使用说明
    ///
    /// 通常不需要直接调用此函数，而是通过全局 `PROCESS_MANAGER` 实例使用：
    ///
    /// ```rust
    /// // 推荐：使用全局实例
    /// use crate::process::manager::{add_process, fetch_process};
    ///
    /// // 不推荐：直接创建实例
    /// let manager = ProcessManager::new();
    /// ```
    ///
    /// ## 线程安全
    ///
    /// `new()` 函数本身是线程安全的，但返回的实例需要通过 `UPSafeCell`
    /// 等同步原语保护才能在多线程环境中安全使用。
    pub fn new() -> Self {
        use crate::config::{MLFQ_BASE_TIME_SLICE, MLFQ_QUEUE_COUNT};

        let mut ready_queues = Vec::new();
        let mut time_slices = Vec::new();

        // 初始化多级队列，时间片按优先级递增
        for i in 0..MLFQ_QUEUE_COUNT {
            ready_queues.push(VecDeque::new());
            // 时间片：10ms, 20ms, 40ms, 80ms
            time_slices.push(MLFQ_BASE_TIME_SLICE * (1 << i));
        }

        Self {
            ready_queues,
            time_slices,
            queue_count: MLFQ_QUEUE_COUNT,
        }
    }

    /// 向就绪队列添加进程
    ///
    /// 将指定的进程控制块添加到就绪队列的尾部，使其可以被调度器选中执行。
    /// 新添加的进程将按照 FIFO 顺序等待调度。
    ///
    /// ## 参数
    ///
    /// * `process` - 要添加到队列的进程控制块，包装在 `Arc` 中支持共享所有权
    ///
    /// ## 调度策略
    ///
    /// 进程按照加入队列的顺序被调度：
    ///
    /// ```text
    /// 调度顺序 (先进先出):
    ///
    /// add(Process1) ──► add(Process2) ──► add(Process3)
    ///      │             │             │
    ///      ▼             ▼             ▼
    /// ┌─────────┐   ┌─────────┐   ┌─────────┐
    /// │Process1 │◄──│ Process2│◄──│ Process3│
    /// │ (First) │   │(Second) │   │ (Last)  │
    /// └─────────┘   └─────────┘   └─────────┘
    ///      ▲                           ▲
    ///      │                           │
    ///   fetch()                     最新添加
    /// ```
    ///
    /// ## 性能特征
    ///
    /// - **时间复杂度**: O(1) 摊还常数时间
    /// - **内存分配**: 队列满时可能触发 O(n) 的扩容操作
    /// - **扩容策略**: 通常按 2 倍容量扩展
    ///
    /// ## 进程状态要求
    ///
    /// 添加到就绪队列的进程通常应该满足：
    /// - 进程状态为 `ProcessStatus::Ready`
    /// - 拥有有效的内存地址空间
    /// - 陷阱上下文已正确初始化
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use alloc::sync::Arc;
    /// use crate::process::process::ProcessControlBlock;
    ///
    /// let process = Arc::new(ProcessControlBlock::new(app_data));
    /// manager.add(process);
    ///
    /// // 进程现在在就绪队列中等待调度
    /// println!("进程已添加到就绪队列");
    /// ```
    ///
    /// ## 并发考虑
    ///
    /// 在并发环境中，应通过适当的同步机制保护此操作：
    ///
    /// ```rust
    /// // 全局接口已提供同步保护
    /// add_process(process); // 推荐方式
    ///
    /// // 直接使用需要手动同步
    /// PROCESS_MANAGER.exclusive_access().add(process);
    /// ```
    /// 向指定优先级队列添加进程
    ///
    /// 将进程添加到指定优先级的就绪队列中。如果优先级超出范围，
    /// 则添加到最低优先级队列。
    ///
    /// ## 参数
    /// * `process` - 要添加的进程控制块
    /// * `priority` - 目标优先级队列（0为最高优先级）
    pub fn add(&mut self, process: Arc<ProcessControlBlock>, priority: usize) {
        let queue_idx = priority.min(self.queue_count - 1);
        self.ready_queues[queue_idx].push_back(process);
    }

    /// 向最高优先级队列添加新进程
    ///
    /// 新创建的进程默认进入最高优先级队列（队列0），
    /// 保证新进程的响应性。
    ///
    /// ## 参数
    /// * `process` - 要添加的进程控制块
    #[allow(unused)]
    pub fn add_new(&mut self, process: Arc<ProcessControlBlock>) {
        self.add(process, 0);
    }

    /// 从就绪队列获取下一个待调度进程
    ///
    /// 从队列头部移除并返回最早加入的进程，实现 FIFO 调度策略。
    /// 如果队列为空，返回 `None`。
    ///
    /// ## 返回值
    ///
    /// * `Some(Arc<ProcessControlBlock>)` - 成功获取到进程控制块
    /// * `None` - 就绪队列为空，没有可调度的进程
    ///
    /// ## 调度行为
    ///
    /// 按照进程加入队列的顺序进行调度：
    ///
    /// ```text
    /// 队列状态变化:
    ///
    /// 调用前:
    /// ┌─────────┐   ┌─────────┐   ┌─────────┐
    /// │ Process1│◄──│Process2 │◄──│ Process3│
    /// │(Oldest) │   │         │   │(Latest) │
    /// └─────────┘   └─────────┘   └─────────┘
    ///      ▲
    ///   fetch() 返回 Process1
    ///
    /// 调用后:
    /// ┌────---─────┐   ┌─────────┐
    /// │  Process2  │◄──│ Process3│
    /// │(Now Oldest)│   │(Latest) │
    /// └───────---──┘   └─────────┘
    /// ```
    ///
    /// ## 性能特征
    ///
    /// - **时间复杂度**: O(1) 常数时间
    /// - **内存操作**: 仅移动指针，不涉及数据拷贝
    /// - **缓存效率**: 访问队列头部具有良好的缓存局部性
    ///
    /// ## 空队列处理
    ///
    /// 当队列为空时安全返回 `None`，调用者应适当处理：
    ///
    /// ```rust
    /// match manager.fetch() {
    ///     Some(process) => {
    ///         println!("获取到进程 PID: {}", process.getpid());
    ///         // 执行进程调度...
    ///     }
    ///     None => {
    ///         println!("没有就绪进程，CPU 进入空闲状态");
    ///         // 可能触发节能模式或等待中断
    ///     }
    /// }
    /// ```
    ///
    /// ## 进程生命周期
    ///
    /// 从队列中取出的进程通常会：
    /// 1. 状态从 `Ready` 转变为 `Running`
    /// 2. 被加载到 CPU 执行
    /// 3. 根据执行结果重新加入队列或结束
    ///
    /// ## 调度公平性
    ///
    /// FIFO 策略保证了调度的公平性：
    /// - 所有进程都有相同的调度机会
    /// - 不会出现进程饥饿问题
    /// - 调度行为完全可预测
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// // 调度器主循环
    /// loop {
    ///     if let Some(process) = manager.fetch() {
    ///         // 切换到进程执行
    ///         run_process(process);
    ///     } else {
    ///         // 没有进程，等待事件
    ///         wait_for_interrupt();
    ///     }
    /// }
    /// ```
    /// 从最高优先级非空队列获取进程
    ///
    /// 按优先级顺序遍历所有队列，从第一个非空队列取出进程。
    /// 实现严格的优先级调度策略。
    ///
    /// ## 返回值
    /// * `Some(Arc<ProcessControlBlock>)` - 成功获取到进程
    /// * `None` - 所有队列都为空
    pub fn fetch(&mut self) -> Option<Arc<ProcessControlBlock>> {
        for queue in &mut self.ready_queues {
            if let Some(process) = queue.pop_front() {
                return Some(process);
            }
        }
        None
    }

    /// 获取指定优先级队列的时间片长度
    ///
    /// ## 参数
    /// * `priority` - 优先级队列索引
    ///
    /// ## 返回值
    /// 该优先级队列的时间片长度（时钟周期数）
    pub fn get_time_slice(&self, priority: usize) -> usize {
        let queue_idx = priority.min(self.queue_count - 1);
        self.time_slices[queue_idx]
    }
}

lazy_static! {
    /// 全局进程管理器实例
    ///
    /// 系统唯一的进程管理器实例，负责维护所有就绪进程的全局状态。
    /// 使用 `UPSafeCell` 包装以提供在单处理器环境下的线程安全访问。
    ///
    /// ## 设计特点
    ///
    /// ### 单例模式
    /// - **全局唯一**: 整个系统只有一个进程管理器实例
    /// - **集中管理**: 所有进程调度操作都通过此实例进行
    /// - **状态一致**: 保证系统进程调度状态的一致性
    ///
    /// ### 并发安全
    /// - **UPSafeCell**: 单处理器环境下的内部可变性
    /// - **互斥访问**: 通过 `exclusive_access()` 获得互斥访问权
    /// - **无竞争**: 在单核系统中避免复杂的锁机制
    ///
    /// ### 延迟初始化
    /// - **lazy_static**: 在首次访问时才执行初始化
    /// - **零成本**: 不会在启动时增加领外开销
    /// - **线程安全**: 初始化过程本身由 lazy_static 保证安全
    ///
    /// ## 内存布局
    ///
    /// ```text
    /// 全局内存区域:
    /// ┌────────────────────────────────────────┐
    /// │            PROCESS_MANAGER             │
    /// ├────────────────────────────────────────┤
    /// │        UPSafeCell<ProcessManager>      │
    /// │  ┌───────────────────────────-─────-┐  │
    /// │  │          ProcessManager          │  │
    /// │  │ ┌──────────────────────────────┐ │  │
    /// │  │ │      VecDeque<Arc<PCB>>      │ │  │
    /// │  │ │    [Process1][Process2] ...  │ │  │
    /// │  │ └──────────────────────────────┘ │  │
    /// │  └─────────────────────────────-───-┘  │
    /// └────────────────────────────────────────┘
    /// ```
    ///
    /// ## 访问模式
    ///
    /// **直接访问**（不推荐）：
    /// ```rust
    /// // 危险！需要手动管理锁
    /// let mut manager = PROCESS_MANAGER.exclusive_access();
    /// manager.add(process);
    /// drop(manager); // 必须显式释放
    /// ```
    ///
    /// **推荐方式**（通过全局函数）：
    /// ```rust
    /// // 安全！自动管理锁生命周期
    /// add_process(process);
    /// let next_process = fetch_process();
    /// ```
    ///
    /// ## 初始化时机
    ///
    /// - **首次访问**: 在第一次调用 `add_process` 或 `fetch_process` 时初始化
    /// - **延迟加载**: 不会影响系统启动速度
    /// - **一次性**: 初始化完成后不会重复执行
    ///
    /// ## 性能影响
    ///
    /// - **全局访问**: 不涉及页表切换，访问速度很快
    /// - **单点争用**: 所有调度操作都会经过此实例
    /// - **缓存局部性**: 全局变量在内存中位置相对固定
    ///
    /// ## 线程安全保证
    ///
    /// - **互斥访问**: `exclusive_access()` 保证同一时间只有一个线程可以修改
    /// - **内存顺序**: 在单核系统中保证操作的原子性
    /// - **无死锁**: 简单的所有权模型避免了复杂的锁依赖
    pub static ref PROCESS_MANAGER: UPSafeCell<ProcessManager> =
        unsafe { UPSafeCell::new(ProcessManager::new()) };

    pub static ref PID2PCB: UPSafeCell<BTreeMap<usize, Arc<ProcessControlBlock>>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

/// 向全局就绪队列添加进程
///
/// - 将 `process` 写入 `PID2PCB` 映射，便于通过 PID 查找 PCB
/// - 将进程入队到全局 `PROCESS_MANAGER` 的就绪队列，等待调度
///
/// ## 参数
/// * `process` - 待加入调度的进程控制块
///
/// ## 复杂度
/// - O(1) 摊还时间；队列扩容时可能触发 O(n) 拷贝
/// 向指定优先级队列添加进程
///
/// 将进程添加到指定优先级的就绪队列中，同时更新 PID 映射。
///
/// ## 参数
/// * `process` - 待加入调度的进程控制块
/// * `priority` - 目标优先级队列（0为最高优先级）
pub fn add_process_with_priority(process: Arc<ProcessControlBlock>, priority: usize) {
    PID2PCB
        .exclusive_access()
        .insert(process.getpid(), Arc::clone(&process));
    PROCESS_MANAGER.exclusive_access().add(process, priority);
}

/// 向最高优先级队列添加新进程
///
/// 新创建的进程默认进入最高优先级队列，保证响应性。
/// 兼容原有的接口，保持向后兼容性。
///
/// ## 参数
/// * `process` - 待加入调度的进程控制块
pub fn add_process(process: Arc<ProcessControlBlock>) {
    add_process_with_priority(process, 0);
}

/// 通过 PID 查询进程控制块
///
/// ## 参数
/// * `pid` - 目标进程标识符
///
/// ## 返回
/// - `Some(Arc<ProcessControlBlock>)`：找到对应进程
/// - `None`：不存在该 PID 的进程（可能已退出并被回收）
pub fn pid2process(pid: usize) -> Option<Arc<ProcessControlBlock>> {
    let map = PID2PCB.exclusive_access();
    map.get(&pid).map(Arc::clone)
}

/// 从全局 PID → PCB 映射中移除进程
///
/// 典型调用点：
/// - 进程退出路径（如 `exit_current_and_run_next` / `waitpid` 回收）
///
/// ## 参数
/// * `pid` - 将要移除的进程标识符
///
/// ## 行为
/// - 若不存在该 PID，触发 panic，用于暴露流程一致性问题
pub fn remove_from_pid2process(pid: usize) {
    let mut map = PID2PCB.exclusive_access();
    if map.remove(&pid).is_none() {
        panic!("cannot find pid {} in pid2process!", pid);
    }
}

/// 从全局进程管理器获取下一个待调度进程
///
/// 调度器核心函数，从就绪队列中获取最早加入的进程进行执行。
/// 实现 FIFO 调度策略，确保进程调度的公平性。
///
/// ## 返回值
///
/// * `Some(Arc<ProcessControlBlock>)` - 成功获取到一个就绪进程
/// * `None` - 就绪队列为空，没有可调度的进程
///
/// ## 调度策略
///
/// 采用先进先出 (FIFO) 策略：
///
/// ```text
/// 调度顺序示意图:
///
/// 时间轴: T1    T2    T3    T4
///         │     │     │     │
///         ▼     ▼     ▼     ▼
///      add(A) add(B) add(C) add(D)
///         │     │     │     │
///         ▼     ▼     ▼     ▼
/// 队列: [A] ─► [B|A] [C|B|A] [D|C|B|A]
///         │           │         │
///         ▼           ▼         ▼
///     fetch()    fetch()   fetch()
///      返回A       返回B     返回C
/// ```
///
/// ## 使用场景
///
/// ### 主调度循环
/// ```rust
/// use crate::process::manager::fetch_process;
///
/// loop {
///     match fetch_process() {
///         Some(process) => {
///             println!("调度进程 PID: {}", process.getpid());
///             run_process(process);
///         }
///         None => {
///             println!("CPU 进入空闲状态");
///             wait_for_interrupt();
///         }
///     }
/// }
/// ```
///
/// ### 批量处理
/// ```rust
/// // 一次取出多个进程进行批量处理
/// let mut processs = Vec::new();
/// while let Some(process) = fetch_process() {
///     processs.push(process);
///     if processs.len() >= BATCH_SIZE {
///         break;
///     }
/// }
/// process_processs_batch(processs);
/// ```
///
/// ### 空闲检测
/// ```rust
/// if fetch_process().is_none() {
///     println!("系统无进程，可以进入节能模式");
///     enter_power_save_mode();
/// }
/// ```
///
/// ## 执行流程
///
/// ```text
/// fetch_process 调用流程:
///
/// Scheduler Code
///      │
///      ▼
/// ┌─────────────────┐
/// │ fetch_process() │ ─── 全局函数接口
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────-┐
/// │exclusive_access()│ ─── 获取互斥访问权
/// └────────┬────────-┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ manager.fetch() │ ─── 从队列头部取出
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ automatic unlock│ ─── 返回后自动释放
/// └─────────────────┘
///          │
///          ▼
///   Option<Arc<PCB>>
/// ```
///
/// ## 性能特征
///
/// - **时间复杂度**: O(1) 常数时间操作
/// - **内存开销**: 仅转移 Arc 所有权，不复制数据
/// - **锁竞争**: 在单核系统中几乎无开销
/// - **缓存友好**: 访问顺序性数据结构
///
/// ## 空队列处理策略
///
/// 当没有可调度进程时的常见处理方式：
///
/// ```rust
/// match fetch_process() {
///     Some(process) => {
///         // 正常调度流程
///         switch_to_process(process);
///     }
///     None => {
///         // 选择合适的空闲处理策略
///         idle_process();           // 执行空闲进程
///         // halt_cpu();         // 挂起 CPU 等待中断
///         // power_management();  // 进入节能模式
///     }
/// }
/// ```
///
/// ## 调度公平性
///
/// FIFO 策略保证了：
/// - **无饥饿**: 没有进程会被无限期推迟
/// - **公平性**: 所有进程都有相等的被调度机会
/// - **可预测**: 调度顺序完全可预测
///
/// ## 错误处理
///
/// 此函数不会返回错误，但可能在以下情况下 panic：
/// - 全局管理器未正确初始化
/// - 队列数据结构损坏（极罕见）
///
/// ## 线程安全
///
/// - **自动锁管理**: 函数进入时自动加锁，返回时自动解锁
/// - **互斥性**: 与 `add_process` 互斥，不会产生竞争条件
/// - **异常安全**: 在 panic 发生时也能正确释放锁
///
/// ## 性能优化建议
///
/// 对于高频调用场景，考虑批量获取优化：
///
/// ```rust
/// // 简单但低效的方式
/// for _ in 0..100 {
///     if let Some(process) = fetch_process() {
///         process_process(process);
///     }
/// }
///
/// // 更高效的批量处理
/// let processs = {
///     let mut manager = PROCESS_MANAGER.exclusive_access();
///     let mut batch = Vec::new();
///     while let Some(process) = manager.fetch() {
///         batch.push(process);
///         if batch.len() >= 100 { break; }
///     }
///     batch
/// };
/// for process in processs {
///     process_process(process);
/// }
/// ```
pub fn fetch_process() -> Option<Arc<ProcessControlBlock>> {
    PROCESS_MANAGER.exclusive_access().fetch()
}

/// 获取指定优先级队列的时间片长度
///
/// ## 参数
/// * `priority` - 优先级队列索引（0为最高优先级）
///
/// ## 返回值
/// 该优先级队列的时间片长度（时钟周期数）
pub fn get_time_slice(priority: usize) -> usize {
    PROCESS_MANAGER.exclusive_access().get_time_slice(priority)
}

/// 提升进程优先级（用于 I/O 操作后）
///
/// 当进程完成 I/O 操作或从阻塞状态恢复时，将其提升到最高优先级队列，
/// 实现 MLFQ 的 I/O 优化策略。
///
/// ## 参数
/// * `process` - 要提升优先级的进程控制块
#[allow(unused)]
pub fn boost_process_priority(process: Arc<ProcessControlBlock>) {
    // 重置为最高优先级
    {
        let mut inner = process.inner_exclusive_access();
        inner.priority = 0;
        inner.time_slice_used = 0;
        inner.time_slice_limit = get_time_slice(0);
    }
    // 重新加入最高优先级队列
    add_process_with_priority(process, 0);
}
