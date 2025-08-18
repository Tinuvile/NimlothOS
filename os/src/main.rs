//! # NimlothOS
//!
//! 一个基于 RISC-V 架构的简单操作系统内核实现
//!
//! ## 主要特性
//!
//! - **任务调度**: 支持多任务协作式调度
//! - **内存管理**: 包含动态堆内存分配器
//! - **系统调用**: 提供基础的系统调用接口
//! - **中断处理**: 支持时钟中断和系统调用
//! - **应用程序加载**: 静态链接多个用户程序
//!
//! ## 模块架构
//!
//! - [`task`] - 任务管理和调度
//! - [`mm`] - 内存管理
//! - [`syscall`] - 系统调用处理
//! - [`trap`] - 中断和异常处理
//! - [`loader`] - 应用程序加载器
//! - [`timer`] - 时钟管理
//! - [`console`] - 控制台输出
//!
//! ## 启动流程
//!
//! 1. 清零 BSS 段
//! 2. 初始化日志系统
//! 3. 初始化中断处理
//! 4. 加载用户应用程序
//! 5. 启用时钟中断
//! 6. 开始任务调度

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
/// 3. [`trap::init`] - 初始化中断处理
/// 4. [`loader::load_apps`] - 加载所有用户应用程序
/// 5. [`trap::enable_timer_interrupt`] - 启用时钟中断
/// 6. [`timer::set_next_trigger`] - 设置第一次时钟中断
/// 7. [`task::run_first_task`] - 开始执行第一个任务
///
/// ## Panics
///
/// 如果 `run_first_task` 意外返回，会触发 panic
#[unsafe(no_mangle)]
pub fn rust_main() -> ! {
    clear_bss();
    log::init();
    info!("[kernel] Hello, world!");

    trap::init();
    loader::load_apps();
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
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe {
        (a as *mut u8).write_volatile(0);
    });
}
