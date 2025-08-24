//! # 物理页帧分配器模块
//!
//! 提供物理页帧的分配和回收功能，管理系统中的可用物理内存页面。
//! 采用栈式分配策略和回收列表，支持高效的页帧分配和自动释放机制。
//!
//! ## 核心组件
//!
//! - [`StackFrameAllocator`] - 栈式物理页帧分配器实现
//! - [`FrameTracker`] - 页帧跟踪器，提供 RAII 自动释放
//! - [`frame_alloc()`] - 全局页帧分配接口
//! - [`init_frame_allocator()`] - 分配器初始化函数
//!
//! ## 分配策略
//!
//! **栈式分配 (Stack Allocation)**：
//! - 从连续的物理页号区间按递增顺序分配
//! - 优先使用回收列表中的页帧（后进先出）
//! - 分配失败时返回 `None`，支持内存不足处理
//!
//! **RAII 管理**：
//! - 通过 [`FrameTracker`] 提供自动页帧释放
//! - 页帧超出作用域时自动回收到分配器
//! - 防止内存泄漏和重复释放
//!
//! ## 内存布局
//!
//! ```text
//! 物理内存:
//! ┌─────────────────────┬─────────────────────-┬─────────────────────┐
//! │   Kernel Image      │  Allocatable Frames  │   Device/Reserved   │
//! │  (Non-allocatable)  │ [ekernel,MEMORY_END) │  (Non-allocatable)  │
//! └─────────────────────┴──────────────-───────┴─────────────────────┘
//!                      ↑                    ↑
//!                   起始页号              结束页号
//! ```
//!
//! ## 使用示例
//!
//! ```rust
//! // 初始化页帧分配器
//! init_frame_allocator();
//!
//! // 分配页帧
//! let frame1 = frame_alloc().expect("内存不足");
//! let frame2 = frame_alloc().expect("内存不足");
//!
//! // 页帧自动释放
//! drop(frame1); // 页帧回到分配器的回收列表
//!
//! // 再次分配可能得到刚释放的页帧
//! let frame3 = frame_alloc(); // 可能重用 frame1 的页帧
//! ```
//!
//! ## 并发安全
//!
//! - 使用 [`UPSafeCell`] 提供线程安全的可变访问
//! - 支持多核环境下的页帧分配操作
//! - 通过独占访问避免竞态条件

use super::{PhysAddr, PhysPageNum};
use crate::sync::UPSafeCell;
use crate::{config::MEMORY_END, println};
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};
use lazy_static::lazy_static;

/// 页帧分配器 trait
///
/// 定义物理页帧分配器的标准接口，支持页帧分配、释放和初始化操作。
/// 所有页帧分配器实现都应该遵循此接口约定。
///
/// ## 设计原则
///
/// - **内存安全**：防止重复释放和使用已释放的页帧
/// - **错误处理**：分配失败时返回 `None`，不触发 panic
/// - **状态管理**：维护分配器内部状态的一致性
///
/// ## 实现要求
///
/// 实现者需要保证：
/// - `alloc()` 返回的页号在有效范围内
/// - `dealloc()` 只接受之前分配的页号
/// - 不会分配同一页帧给多个调用者
trait FrameAllocator {
    /// 创建新的分配器实例
    ///
    /// 创建一个未初始化的分配器，需要调用 `init()` 方法
    /// 设置可分配的页帧范围。
    ///
    /// ## Returns
    ///
    /// 返回新创建的分配器实例
    fn new() -> Self;

    /// 分配一个物理页帧
    ///
    /// 从可用页帧池中分配一个 4KB 物理页帧。优先使用
    /// 回收列表中的页帧，其次按顺序分配新页帧。
    ///
    /// ## Returns
    ///
    /// - `Some(ppn)` - 成功分配的物理页号
    /// - `None` - 内存不足，分配失败
    ///
    /// ## 分配策略
    ///
    /// 1. 检查回收列表，优先复用已释放的页帧
    /// 2. 如果回收列表为空，从连续区间分配新页帧
    /// 3. 如果所有页帧都已分配，返回 `None`
    fn alloc(&mut self) -> Option<PhysPageNum>;

    /// 释放一个物理页帧
    ///
    /// 将之前分配的页帧回收到分配器，使其可以被再次分配。
    /// 释放的页帧会被添加到回收列表中。
    ///
    /// ## Arguments
    ///
    /// * `ppn` - 要释放的物理页号，必须是之前通过 `alloc()` 获得的
    ///
    /// ## Panics
    ///
    /// 以下情况会触发 panic：
    /// - 释放未分配的页帧
    /// - 重复释放同一页帧
    /// - 释放无效的页号（超出分配器管理范围）
    fn dealloc(&mut self, ppn: PhysPageNum);
}

/// 栈式页帧分配器
///
/// 采用栈式策略的物理页帧分配器实现，维护连续的可分配页帧区间
/// 和一个回收页帧的动态列表。
///
/// ## 分配机制
///
/// **连续分配区间**：
/// - `current`：下一个待分配的页号
/// - `end`：分配区间的结束页号（不包含）
/// - 新页帧按 `current` 递增顺序分配
///
/// **回收列表**：
/// - `recycled`：存储已释放页帧的动态数组
/// - 分配时优先从回收列表弹出页帧（LIFO）
/// - 释放时将页帧推入回收列表
///
/// ## 内存复杂度
///
/// - **空间复杂度**：O(R)，R 为回收列表中的页帧数
/// - **时间复杂度**：分配和释放均为 O(1)
///
/// ## 初始化状态
///
/// 新创建的分配器所有字段都为 0/空，需要调用 [`init()`] 方法
/// 设置实际的页帧分配范围。
pub struct StackFrameAllocator {
    /// 下一个待分配的页号
    ///
    /// 指向连续分配区间中下一个可用的物理页号。
    /// 每次分配后递增，直到达到 `end`。
    current: usize,

    /// 分配区间结束页号（不包含）
    ///
    /// 标记连续分配区间的上界，当 `current == end` 时
    /// 表示连续区间已用完，只能从回收列表分配。
    end: usize,

    /// 回收页帧列表
    ///
    /// 存储已释放页帧的页号，采用 LIFO（后进先出）策略。
    /// 分配时优先从此列表弹出页帧，提高内存局部性。
    recycled: Vec<usize>,
}

impl StackFrameAllocator {
    /// 初始化页帧分配器
    ///
    /// 设置分配器的可分配页帧范围，该范围通常是从内核镜像结束
    /// 到物理内存结束的连续物理页面。
    ///
    /// ## Arguments
    ///
    /// * `l` - 起始物理页号（包含）
    /// * `r` - 结束物理页号（不包含）
    ///
    /// ## 分配范围
    ///
    /// 分配器将管理区间 `[l, r)` 内的所有物理页帧：
    /// - 总共 `r - l` 个可分配页帧
    /// - 每个页帧大小为 4KB (PAGE_SIZE)
    /// - 页帧按页号递增顺序分配
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let start_ppn = PhysAddr::from(0x80400000).ceil(); // 内核后第一页
    /// let end_ppn = PhysAddr::from(0x88000000).floor();  // 物理内存最后一页
    /// allocator.init(start_ppn, end_ppn);
    /// // 现在可以分配 [start_ppn, end_ppn) 范围内的页帧
    /// ```
    ///
    /// ## Postconditions
    ///
    /// 初始化后分配器状态：
    /// - `current = l.0`：从起始页号开始分配
    /// - `end = r.0`：设置分配上界  
    /// - `recycled` 保持空列表
    pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
        self.current = l.0;
        self.end = r.0;
    }
}

impl FrameAllocator for StackFrameAllocator {
    /// 创建新的栈式分配器实例
    ///
    /// 创建一个空的分配器实例，所有字段都初始化为 0 或空状态。
    /// 需要调用 [`init()`] 方法才能开始分配页帧。
    ///
    /// ## Returns
    ///
    /// 返回未初始化的分配器实例
    fn new() -> Self {
        Self {
            current: 0,
            end: 0,
            recycled: Vec::new(),
        }
    }

    /// 分配一个物理页帧
    ///
    /// 实现栈式分配策略：优先从回收列表弹出页帧，如果回收列表为空
    /// 则从连续区间分配新页帧。
    ///
    /// ## Returns
    ///
    /// - `Some(ppn)` - 成功分配的物理页号
    /// - `None` - 内存耗尽，无可用页帧
    ///
    /// ## 分配逻辑
    ///
    /// 1. **回收列表优先**：如果 `recycled` 非空，弹出最后一个页号
    /// 2. **连续分配**：如果回收列表为空且 `current < end`，分配 `current` 页号并递增
    /// 3. **分配失败**：如果连续区间耗尽且回收列表为空，返回 `None`
    ///
    /// ## 时间复杂度
    ///
    /// O(1) - 所有操作都是常数时间
    fn alloc(&mut self) -> Option<PhysPageNum> {
        if let Some(ppn) = self.recycled.pop() {
            Some(ppn.into())
        } else {
            if self.current == self.end {
                None
            } else {
                self.current += 1;
                Some((self.current - 1).into())
            }
        }
    }

    /// 释放一个物理页帧
    ///
    /// 将页帧回收到回收列表中，使其可以被再次分配。进行安全性检查
    /// 防止释放无效或重复释放的页帧。
    ///
    /// ## Arguments
    ///
    /// * `ppn` - 要释放的物理页号
    ///
    /// ## 安全性检查
    ///
    /// - **有效性检查**：页号必须小于 `current`（已分配过）
    /// - **重复释放检查**：页号不能已存在于回收列表中
    ///
    /// ## Panics
    ///
    /// 在以下情况下会触发 panic：
    /// - `ppn >= current`：释放未分配的页帧
    /// - `ppn` 已存在于回收列表：重复释放同一页帧
    ///
    /// ## 时间复杂度
    ///
    /// O(R) - R 为回收列表长度，用于重复释放检查
    fn dealloc(&mut self, ppn: PhysPageNum) {
        let ppn = ppn.0;
        if ppn >= self.current || self.recycled.iter().find(|&v| *v == ppn).is_some() {
            panic!("Frame ppn={:#x} has not been allocated", ppn);
        }
        self.recycled.push(ppn);
    }
}

/// 全局页帧分配器实现类型别名
///
/// 当前使用 [`StackFrameAllocator`] 作为系统的页帧分配器实现。
/// 可以通过修改此别名切换到其他分配器实现。
type FrameAllocatorImpl = StackFrameAllocator;

lazy_static! {
    /// 全局页帧分配器实例
    ///
    /// 系统唯一的物理页帧分配器，使用 [`UPSafeCell`] 提供线程安全的
    /// 可变访问。所有页帧分配操作都通过此实例进行。
    ///
    /// ## 并发安全
    ///
    /// - 使用 [`UPSafeCell::exclusive_access()`] 获取独占访问权限
    /// - 支持多核环境下的安全并发访问
    /// - 避免数据竞争和状态不一致
    ///
    /// ## 初始化
    ///
    /// 在系统启动时需要调用 [`init_frame_allocator()`] 进行初始化，
    /// 设置可分配的物理页帧范围。
    pub static ref FRAME_ALLOCATOR: UPSafeCell<FrameAllocatorImpl> =
        unsafe { UPSafeCell::new(FrameAllocatorImpl::new()) };
}

/// 初始化全局页帧分配器
///
/// 在系统启动时调用，设置页帧分配器的可分配物理内存范围。
/// 分配范围从内核镜像结束处到物理内存末尾。
///
/// ## 分配范围计算
///
/// **起始地址**：`ekernel` 符号地址向上对齐到页边界
/// - `ekernel`：链接器定义的内核镜像结束符号
/// - [`PhysAddr::ceil()`]：向上对齐避免覆盖内核数据
///
/// **结束地址**：`MEMORY_END` 常量向下对齐到页边界
/// - `MEMORY_END`：板级配置定义的物理内存结束地址
/// - [`PhysAddr::floor()`]：向下对齐避免越界访问
///
/// ## 内存布局
///
/// ```text
/// 物理内存:
/// ┌─────────────────┬─────────────────┬───────────────┐
/// │  Kernel Image   │ Allocatable PPN │ Device/Reserve│
/// │  [0, ekernel)   │ [ekernel, END)  │  [END, MAX)   │
/// └─────────────────┴─────────────────┴───────────────┘
///                  ↑                 ↑
///            ceil(ekernel)      floor(END)
/// ```
///
/// ## 调用时机
///
/// 必须在以下之前调用：
/// - 任何页帧分配操作 ([`frame_alloc()`])
/// - 内存管理子系统初始化
/// - 页表创建和映射操作
///
/// ## Safety
///
/// 使用 `unsafe extern "C"` 访问链接器符号 `ekernel`，
/// 调用者需要确保链接脚本正确定义了该符号。
pub fn init_frame_allocator() {
    unsafe extern "C" {
        safe fn ekernel();
    }
    FRAME_ALLOCATOR.exclusive_access().init(
        PhysAddr::from(ekernel as usize).ceil(),
        PhysAddr::from(MEMORY_END).floor(),
    );
}

/// 页帧跟踪器 (Frame Tracker)
///
/// 提供物理页帧的 RAII (Resource Acquisition Is Initialization) 管理，
/// 确保页帧在不再使用时自动释放，防止内存泄漏。
///
/// ## RAII 机制
///
/// - **获取时初始化**：创建时自动清零页面内容
/// - **作用域管理**：离开作用域时自动释放页帧
/// - **所有权语义**：独占拥有一个物理页帧
///
/// ## 生命周期
///
/// 1. **创建**：通过 [`frame_alloc()`] 分配并初始化
/// 2. **使用**：通过 `ppn` 字段访问物理页号
/// 3. **释放**：超出作用域时自动调用 [`Drop::drop()`]
///
/// ## 内存安全
///
/// - **防止泄漏**：确保每个分配的页帧都会被释放
/// - **防止重复释放**：每个 `FrameTracker` 只释放一次
/// - **初始化保证**：新分配的页面内容都清零
///
/// ## 使用示例
///
/// ```rust
/// // 分配页帧，内容自动清零
/// let frame = frame_alloc().expect("内存不足");
///
/// // 通过物理页号访问页面数据
/// let page_data = frame.ppn.bytes_array();
/// page_data[0] = 42;
///
/// // 离开作用域时页帧自动释放
/// drop(frame); // 显式释放，也可以等待自动释放
/// ```
pub struct FrameTracker {
    /// 跟踪的物理页号
    ///
    /// 该页帧的唯一标识，可用于访问 4KB 物理页面的内容。
    /// 当 `FrameTracker` 被销毁时，此页号对应的页帧会被自动释放。
    pub ppn: PhysPageNum,
}

impl FrameTracker {
    /// 创建新的页帧跟踪器
    ///
    /// 接管指定物理页帧的管理权，并将页面内容全部清零。
    /// 通常不直接调用，而是通过 [`frame_alloc()`] 间接创建。
    ///
    /// ## Arguments
    ///
    /// * `ppn` - 要跟踪的物理页号
    ///
    /// ## Returns
    ///
    /// 返回新创建的页帧跟踪器
    ///
    /// ## 初始化过程
    ///
    /// 1. 获取页面的字节数组访问权限
    /// 2. 将 4096 字节全部设置为 0
    /// 3. 创建跟踪器实例
    ///
    /// ## 时间复杂度
    ///
    /// O(PAGE_SIZE) - 需要清零 4096 字节
    ///
    /// ## Safety
    ///
    /// 调用者需要确保：
    /// - `ppn` 对应有效的已分配页帧
    /// - 没有其他代码同时访问同一页帧
    pub fn new(ppn: PhysPageNum) -> Self {
        let bytes_array = ppn.bytes_array();
        for i in bytes_array {
            *i = 0;
        }
        Self { ppn }
    }
}

/// 分配一个物理页帧
///
/// 全局页帧分配接口，从系统页帧池中分配一个 4KB 物理页面，
/// 并返回一个 [`FrameTracker`] 进行 RAII 管理。
///
/// ## Returns
///
/// - `Some(FrameTracker)` - 成功分配的页帧跟踪器
/// - `None` - 系统内存不足，分配失败
///
/// ## 分配流程
///
/// 1. 获取全局分配器的独占访问权限
/// 2. 调用底层分配器的 `alloc()` 方法
/// 3. 如果成功，创建 `FrameTracker` 并清零页面
/// 4. 返回跟踪器或 `None`
///
/// ## 并发安全
///
/// 通过 [`UPSafeCell::exclusive_access()`] 确保线程安全，
/// 同一时刻只有一个线程可以访问分配器。
///
/// ## 使用示例
///
/// ```rust
/// match frame_alloc() {
///     Some(frame) => {
///         println!("分配成功: {:?}", frame);
///         // 使用页帧...
///     }
///     None => {
///         panic!("系统内存不足！");
///     }
/// }
/// ```
///
/// ## 错误处理
///
/// 分配失败通常表示：
/// - 物理内存耗尽
/// - 分配器未正确初始化
/// - 内存碎片过多
pub fn frame_alloc() -> Option<FrameTracker> {
    FRAME_ALLOCATOR
        .exclusive_access()
        .alloc()
        .map(|ppn| FrameTracker::new(ppn))
}

/// 释放物理页帧（内部函数）
///
/// 将指定的物理页帧回收到全局分配器中，使其可以被再次分配。
/// 通常不直接调用，而是通过 `FrameTracker` 的 `Drop` 实现自动调用。
///
/// ## Arguments
///
/// * `ppn` - 要释放的物理页号
///
/// ## 并发安全
///
/// 通过全局分配器的独占访问机制确保线程安全。
///
/// ## 调用时机
///
/// 主要在以下场景调用：
/// - `FrameTracker::drop()` 实现中
/// - 错误处理路径中的手动清理
pub fn frame_dealloc(ppn: PhysPageNum) {
    FRAME_ALLOCATOR.exclusive_access().dealloc(ppn);
}

impl Drop for FrameTracker {
    /// 页帧跟踪器析构函数
    ///
    /// 当 `FrameTracker` 超出作用域时自动调用，负责将管理的
    /// 物理页帧释放回分配器，实现 RAII 的"获取即初始化"机制。
    ///
    /// ## 释放过程
    ///
    /// 1. 自动调用 [`frame_dealloc()`] 函数
    /// 2. 将页号回收到全局分配器的回收列表
    /// 3. 页帧可以被后续的分配请求重用
    ///
    /// ## 调用时机
    ///
    /// - 显式调用 `drop(frame)`
    /// - 变量超出作用域
    /// - 容器被清空 (`Vec::clear()`)
    /// - 程序异常终止时的清理
    ///
    /// ## 内存安全保证
    ///
    /// - 每个页帧有且仅有一个 `FrameTracker` 管理
    /// - 释放操作是原子的，不会产生竞态条件
    /// - 释放后的页帧不会被意外访问
    fn drop(&mut self) {
        frame_dealloc(self.ppn);
    }
}

impl Debug for FrameTracker {
    /// 格式化输出页帧跟踪器的调试信息
    ///
    /// 提供页帧跟踪器的可读性调试输出，显示所管理的物理页号。
    /// 主要用于调试和日志记录。
    ///
    /// ## 输出格式
    ///
    /// ```text
    /// FrameTracker: PPN=0x12345
    /// ```
    ///
    /// ## Arguments
    ///
    /// * `f` - 格式化器，用于输出调试信息
    ///
    /// ## Returns
    ///
    /// 格式化结果，成功时返回 `Ok(())`
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("FrameTracker: PPN={:#x}", self.ppn.0))
    }
}

/// 页帧分配器功能测试
///
/// 验证页帧分配器的基本功能，包括分配、RAII 自动释放和回收复用。
/// 用于确保分配器实现的正确性。
///
/// ## 测试流程
///
/// **阶段1：初始分配测试**
/// 1. 连续分配 5 个页帧并打印信息
/// 2. 验证每次分配返回不同的页号
/// 3. 将分配的页帧存储在向量中
///
/// **阶段2：释放回收测试**
/// 1. 清空向量，触发所有 `FrameTracker` 的 `drop`
/// 2. 页帧被自动释放到回收列表
///
/// **阶段3：复用测试**
/// 1. 再次分配 5 个页帧
/// 2. 验证分配器复用了之前释放的页帧
/// 3. 检查分配策略的 LIFO 特性
///
/// ## 验证要点
///
/// - **分配成功**：所有分配请求都应该成功
/// - **页号唯一**：同时存在的页帧页号不重复  
/// - **自动释放**：`Vec::clear()` 后页帧自动回收
/// - **回收复用**：第二轮分配复用第一轮的页帧
/// - **LIFO 顺序**：后释放的页帧先被重新分配
///
/// ## 使用方法
///
/// ```rust
/// // 在系统初始化后调用
/// init_frame_allocator();
/// frame_alloctor_test(); // 输出: "frame_allocator_test passed!"
/// ```
///
/// ## 输出示例
///
/// ```text
/// FrameTracker: PPN=0x80401
/// FrameTracker: PPN=0x80402  
/// FrameTracker: PPN=0x80403
/// FrameTracker: PPN=0x80404
/// FrameTracker: PPN=0x80405
/// FrameTracker: PPN=0x80405  // 复用最后释放的页帧
/// FrameTracker: PPN=0x80404  // LIFO 顺序
/// FrameTracker: PPN=0x80403
/// FrameTracker: PPN=0x80402
/// FrameTracker: PPN=0x80401
/// frame_allocator_test passed!
/// ```
#[allow(unused)]
pub fn frame_alloctor_test() {
    let mut v: Vec<FrameTracker> = Vec::new();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    v.clear();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    drop(v);
    println!("frame_allocator_test passed!")
}
