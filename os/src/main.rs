//! # NimlothOS
//!
//! 一个基于 RISC-V 架构的简单操作系统内核实现
//!
//! ## 主要特性
//!
//! - **多任务支持**: 基于时间片轮转的抢占式多任务调度
//! - **内存管理**: SV39 三级页表，支持虚拟内存和地址空间隔离
//! - **系统调用**: 支持 write、exit、yield、get_time、sbrk 等系统调用
//! - **陷阱处理**: 完整的异常、中断和系统调用处理机制
//! - **应用加载**: 支持从内核镜像中加载多个用户应用程序
//!
//! ## 模块架构
//!
//! - [`task`] - 任务管理和调度系统
//! - [`mm`] - 内存管理系统（页表、页帧分配、地址空间）
//! - [`syscall`] - 系统调用处理和分发
//! - [`trap`] - 陷阱处理（异常、中断、系统调用）
//! - [`loader`] - 应用程序加载和管理
//! - [`timer`] - 时钟管理和定时中断
//! - [`console`] - 控制台输入输出
//! - [`sync`] - 同步原语（UPSafeCell 等）
//! - [`config`] - 系统配置常量
//! - [`sbi`] - SBI 接口封装
//!
//! ## 系统架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    User Applications                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    System Calls                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Task Mgmt  │  Memory Mgmt  │  Trap Handler │  Timer Mgmt   │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   SBI Interface                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   RISC-V Hardware                           │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

#[macro_use]
mod config;
mod console;
mod lang_items;
mod loader;
mod log;
mod mm;
mod sbi;
mod stack_trace;
mod sync;
mod syscall;
mod task;
mod timer;
mod trap;

#[path = "board/qemu.rs"]
mod board;

use core::arch::global_asm;

extern crate alloc;
extern crate bitflags;

global_asm!(include_str!("entry.asm"));
global_asm!(include_str!("link_app.S"));

/// 内核主入口函数
///
/// 这是 Rust 代码的主要入口点，负责初始化整个操作系统内核。
/// 该函数永不返回，最终会转移控制权给第一个用户任务。
///
/// ## 初始化流程
///
/// 1. [`clear_bss`] - 清零 BSS 段
/// 2. [`log::init`] - 初始化日志系统
/// 3. [`mm::init`] - 初始化内存管理系统
/// 4. [`mm::remap_test`] - 测试内存重映射功能
/// 5. [`trap::init`] - 初始化陷阱处理系统
/// 6. [`trap::enable_timer_interrupt`] - 启用时钟中断
/// 7. [`timer::set_next_trigger`] - 设置第一次时钟中断
/// 8. [`task::run_first_task`] - 开始执行第一个任务
///
/// ## Panics
///
/// 如果 `run_first_task` 意外返回，会触发 panic
#[unsafe(no_mangle)]
pub fn rust_main() -> ! {
    clear_bss();
    log::init();
    info!("[kernel] Hello, world!");
    mm::init();
    info!("[kernel] back to world!");
    mm::remap_test();
    trap::init();
    trap::enable_timer_interrupt();
    timer::set_next_trigger();
    task::run_first_task();

    panic!("Unreachable in rust_main!");
}

/// 清零 BSS 段
///
/// BSS 段包含程序中未初始化的全局变量和静态变量。
/// 根据 C 标准和 Rust 语义，这些变量应该被初始化为零。
///
/// 该函数遍历从 `sbss` 到 `ebss` 的内存区域，将每个字节设置为 0。
/// 这些符号由链接器脚本 (`linker-qemu.ld`) 定义。
///
/// ## Safety
///
/// 该函数使用 `unsafe` 代码：
/// - 调用外部 C 函数符号 `sbss` 和 `ebss`
/// - 直接操作内存地址
/// - 使用 `write_volatile` 确保编译器不会优化掉写操作
///
/// 这个函数必须在任何 Rust 代码使用全局变量之前调用。
fn clear_bss() {
    unsafe extern "C" {
        safe fn sbss();
        safe fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe {
        (a as *mut u8).write_volatile(0);
    });
}
