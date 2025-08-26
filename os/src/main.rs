//! # NimlothOS
//!
//! 一个基于 RISC-V 架构的简单操作系统内核实现
//!
//! ## 主要特性
//!
//! - **多进程支持**: 基于时间片轮转的抢占式多进程调度
//! - **内存管理**: SV39 三级页表，支持虚拟内存和地址空间隔离
//! - **系统调用**: 支持 read、write、exit、yield、time、pid、fork、exec、waitpid 等系统调用
//! - **陷阱处理**: 完整的异常、中断和系统调用处理机制
//! - **应用加载**: 支持从内核镜像中加载多个用户应用程序
//!
//! ## 模块架构
//!
//! - [`process`] - 进程管理和调度系统
//! - [`mm`] - 内存管理系统（页表、页帧分配、地址空间）
//! - [`syscall`] - 系统调用处理和分发
//! - [`trap`] - 陷阱处理（异常、中断、系统调用）
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
//! │  Process Mgmt  │  Memory Mgmt  │  Trap Handler │  Timer Mgmt   │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   SBI Interface                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   RISC-V Hardware                           │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![doc(test(no_crate_inject))]

#[macro_use]
mod config;
mod console;
mod drivers;
mod fs;
mod lang_items;
mod log;
mod mm;
mod process;
mod sbi;
mod stack_trace;
mod sync;
mod syscall;
mod timer;
mod trap;

#[path = "board/qemu.rs"]
mod board;

use core::arch::global_asm;

extern crate alloc;
extern crate bitflags;

global_asm!(include_str!("entry.asm"));

/// 内核主入口函数
///
/// 这是 Rust 代码的主要入口点，负责初始化整个操作系统内核。
/// 该函数永不返回，最终进入调度循环持续运行用户进程。
///
/// ## 初始化流程
///
/// 1. [`clear_bss`] - 清零 BSS 段
/// 2. [`log::init`] - 初始化日志系统
/// 3. [`mm::init`] - 初始化内存管理系统
/// 4. [`mm::remap_test`] - 测试内存重映射功能
/// 5. [`process::add_initproc`] - 注册初始用户进程
/// 6. [`trap::init`] - 初始化陷阱处理系统
/// 7. [`timer::next_trigger`] - 设置第一次时钟中断
/// 8. [`process::run_processs`] - 进入主调度循环
///
/// ## Panics
///
/// 如果 `run_first_process` 意外返回，会触发 panic
#[unsafe(no_mangle)]
pub fn rust_main() -> ! {
    clear_bss();
    log::init();
    ::log::info!("[kernel] Hello, world!");
    mm::init();
    trap::init();
    timer::next_trigger();
    fs::list_apps();
    process::add_initproc();
    process::run_processs();

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
    unsafe {
        core::slice::from_raw_parts_mut(sbss as usize as *mut u8, ebss as usize - sbss as usize)
            .fill(0);
    }
}
