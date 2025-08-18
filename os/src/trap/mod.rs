//! # 陷阱和中断处理模块
//!
//! 处理所有从用户态到内核态的陷阱 (trap)，包括系统调用、异常和中断。
//! 陷阱是 RISC-V 架构中用户态与内核态交互的核心机制。
//!
//! ## 处理的陷阱类型
//!
//! - **系统调用** (`UserEnvCall`): 用户程序请求内核服务
//! - **时钟中断** (`SupervisorTimer`): 实现抢占式多任务调度
//! - **页面异常** (`StoreFault`, `StorePageFault`): 内存访问违规
//! - **非法指令** (`IllegalInstruction`): 执行无效指令
//!
//! ## 执行流程
//!
//! 1. **陷阱触发**: 用户态程序执行 `ecall` 或发生异常/中断
//! 2. **硬件切换**: CPU 自动切换到 S 模式，跳转到 `stvec` 指定的处理程序
//! 3. **上下文保存**: `__alltraps` 保存所有寄存器到陷阱上下文
//! 4. **处理分发**: `trap_handler` 根据陷阱类型执行相应处理
//! 5. **上下文恢复**: `__restore` 恢复寄存器并返回用户态
//!
//! ## 寄存器使用
//!
//! - `stvec`: 陷阱向量寄存器，指向陷阱处理入口 `__alltraps`
//! - `scause`: 陷阱原因寄存器，标识陷阱类型和具体原因
//! - `stval`: 陷阱值寄存器，包含相关的地址或值信息
//! - `sstatus`: 状态寄存器，控制中断使能和特权级
//! - `sepc`: 异常程序计数器，指向触发陷阱的指令地址

use crate::syscall::syscall;
use crate::task::exit_current_and_run_next;
use crate::timer::set_next_trigger;
use crate::{println, task::suspend_current_and_run_next};
use core::arch::global_asm;
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Interrupt, Trap},
    sie, stval, stvec,
};

mod context;

// 包含汇编实现的陷阱处理代码
global_asm!(include_str!("trap.S"));

/// 初始化陷阱处理系统
///
/// 设置陷阱向量寄存器 `stvec`，指向陷阱处理入口点 `__alltraps`。
/// 必须在系统启动早期调用，在任何可能触发陷阱的操作之前。
///
/// ## 配置内容
///
/// - 将 `stvec` 设置为 `__alltraps` 函数地址
/// - 使用直接模式 (`TrapMode::Direct`)，所有陷阱都跳转到同一个处理程序
///
/// ## Safety
///
/// 此函数是安全的，但内部使用 `unsafe` 访问 CSR 寄存器。
pub fn init() {
    unsafe extern "C" {
        fn __alltraps();
    }
    unsafe {
        stvec::write(__alltraps as usize, TrapMode::Direct);
    }
}

/// 启用时钟中断
///
/// 在监督者中断使能寄存器 (`sie`) 中启用时钟中断位，
/// 允许时钟中断触发陷阱进入内核进行任务调度。
///
/// ## 功能
///
/// - 设置 `sie.STIE` 位，启用监督者时钟中断
/// - 配合时钟设备实现抢占式多任务调度
/// - 必须在时钟设备配置完成后调用
///
/// ## Safety
///
/// 此函数是安全的，但内部使用 `unsafe` 访问 CSR 寄存器。
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}

/// 陷阱处理器主函数
///
/// 这是所有陷阱的统一处理入口，由汇编代码 `__alltraps` 调用。
/// 根据陷阱类型分发到不同的处理逻辑，处理完成后返回修改后的陷阱上下文。
///
/// ## Arguments
///
/// * `cx` - 陷阱上下文的可变引用，包含触发陷阱时的 CPU 状态
///
/// ## Returns
///
/// 返回修改后的陷阱上下文，供 `__restore` 恢复到用户态
///
/// ## 处理的陷阱类型
///
/// ### 1. 系统调用 (`UserEnvCall`)
///
/// - 将 `sepc` 加 4，跳过 `ecall` 指令
/// - 从寄存器提取系统调用号和参数
/// - 调用 [`syscall`] 执行具体的系统调用
/// - 将返回值写入 `a0` 寄存器 (`cx.x[10]`)
///
/// ### 2. 存储异常 (`StoreFault`, `StorePageFault`)
///
/// - 记录故障地址 (`stval`) 和指令地址 (`sepc`)
/// - 输出错误信息到内核日志
/// - 终止当前任务并调度下一个任务
///
/// ### 3. 非法指令 (`IllegalInstruction`)
///
/// - 记录异常指令地址 (`sepc`)
/// - 输出错误信息到内核日志  
/// - 终止当前任务并调度下一个任务
///
/// ### 4. 监督者时钟中断 (`SupervisorTimer`)
///
/// - 设置下一次时钟中断触发时间
/// - 挂起当前任务并切换到下一个就绪任务
/// - 实现抢占式多任务调度
///
/// ## 寄存器约定
///
/// 遵循 RISC-V ABI 约定：
/// - `x10` (`a0`): 系统调用返回值 / 第一个参数
/// - `x11` (`a1`): 系统调用第二个参数
/// - `x12` (`a2`): 系统调用第三个参数  
/// - `x17` (`a7`): 系统调用号
///
/// ## 执行流程
///
/// ```text
/// 陷阱触发 -> __alltraps -> trap_handler -> __restore -> 用户态
/// ```
///
/// ## Safety
///
/// - 使用 `#[unsafe(no_mangle)]` 确保函数名不被修改，供汇编代码调用
/// - 函数本身是安全的，所有 unsafe 操作都在子函数中处理
///
/// ## Panics
///
/// 遇到不支持的陷阱类型时会触发 panic，这通常表示：
/// - 硬件故障
/// - 用户程序触发了预期外的异常
/// - 内核配置错误
#[unsafe(no_mangle)]
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    let scause = scause::read();
    let stval = stval::read();
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // 系统调用处理
            cx.sepc += 4; // 跳过 ecall 指令
            cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
        }
        Trap::Exception(Exception::StoreFault) | Trap::Exception(Exception::StorePageFault) => {
            // 存储异常：非法内存访问
            println!(
                "[kernel] PageFault in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.",
                stval, cx.sepc
            );
            exit_current_and_run_next();
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            // 非法指令异常：执行了无效的指令
            println!(
                "[kernel] IllegalInstruction in application, bad instruction = {:#x}, kernel killed it.",
                cx.sepc
            );
            exit_current_and_run_next();
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            // 时钟中断：实现抢占式调度
            set_next_trigger();
            suspend_current_and_run_next();
        }
        _ => {
            // 未处理的陷阱类型
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval,
            );
        }
    }
    cx
}

pub use context::TrapContext;
