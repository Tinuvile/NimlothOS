//! # 内核配置常量
//!
//! 定义了操作系统内核使用的各种配置参数，包括内存布局、栈大小、
//! 应用程序限制等关键系统参数。

/// 用户栈大小 (8KB)
///
/// 每个用户应用程序的栈大小，用于存储用户态的函数调用栈、
/// 局部变量和函数参数。
pub const USER_STACK_SIZE: usize = 4096 * 2;

/// 内核栈大小 (8KB)
///
/// 每个任务在内核态执行时使用的栈大小，用于处理系统调用、
/// 中断和异常时的函数调用。
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;

/// 内核堆大小 (3MB)
///
/// 内核动态内存分配器的总大小，用于支持 `Box`、`Vec` 等
/// 需要堆内存的数据结构。
pub const KERNEL_HEAP_SIZE: usize = 0x30_0000;

/// 页面大小 (4KB)
///
/// 内存管理单元 (MMU) 的基本页面大小，符合 RISC-V 标准。
pub const PAGE_SIZE: usize = 0x1000;

/// 页面大小的位数 (12 位)
///
/// 用于地址计算中的位移操作，4KB = 2^12 字节。
pub const PAGE_SIZE_BITS: usize = 0xc;

/// 跳板页地址
///
/// 位于虚拟地址空间的最高页面，用于在用户态和内核态之间
/// 进行上下文切换。所有应用程序共享同一个跳板页。
pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;

/// 陷阱上下文页地址
///
/// 紧邻跳板页的下方，用于存储每个应用程序的陷阱上下文，
/// 包括寄存器状态等信息。
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;

/// 计算指定应用程序的内核栈位置
///
/// 每个应用程序在虚拟地址空间中都有独立的内核栈，用于处理该应用程序
/// 的系统调用和中断。内核栈从高地址向低地址增长，位于跳板页之下。
///
/// ## Arguments
///
/// * `app_id` - 应用程序 ID，从 0 开始编号
///
/// ## Returns
///
/// 返回一个元组 `(bottom, top)`，表示该应用程序内核栈的地址范围：
/// - `bottom` - 栈底地址（低地址）
/// - `top` - 栈顶地址（高地址）
///
/// ## 内存布局
///
/// ```text
/// 高地址 -> TRAMPOLINE (跳板页)
///          |-- PAGE_SIZE (GUARD PAGE) --|
///          |--   KERNEL_STACK_SIZE    --|  <- app 0 的内核栈
///          |-- PAGE_SIZE (GUARD PAGE) --|
///          |--   KERNEL_STACK_SIZE    --|  <- app 1 的内核栈
///          ...
/// 低地址
/// ```
#[allow(unused)]
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

// 从开发板配置中重新导出时钟频率和内存信息
pub use crate::board::{CLOCK_FREQ, MEMORY_END, MMIO};
