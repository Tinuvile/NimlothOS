//! # 陷阱和中断处理模块
//!
//! 处理所有从用户态到内核态的陷阱 (trap)，包括系统调用、异常和中断。
//! 陷阱是 RISC-V 架构中用户态与内核态交互的核心机制。
//!
//! ## 处理的陷阱类型
//!
//! - **系统调用** (`UserEnvCall`): 用户程序请求内核服务
//! - **时钟中断** (`SupervisorTimer`): 实现抢占式多任务调度
//! - **数据访问异常** (`StoreFault`, `StorePageFault`, `LoadFault`, `LoadPageFault`): 数据内存访问违规
//! - **指令访问异常** (`InstructionFault`, `InstructionPageFault`): 指令内存访问违规
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

use crate::config::{TRAMPOLINE, TRAP_CONTEXT};
use crate::syscall::syscall;
use crate::task::{current_trap_cx, current_user_token, exit_current_and_run_next};
use crate::timer::set_next_trigger;
use crate::{println, task::suspend_current_and_run_next};
use core::arch::{asm, global_asm};
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Interrupt, Trap},
    sie, stval, stvec,
};

pub use context::TrapContext;

mod context;

// 包含汇编实现的陷阱处理代码
global_asm!(include_str!("trap.S"));

/// 初始化陷阱处理系统
///
/// 设置内核态的陷阱处理入口点，配置陷阱向量寄存器 (`stvec`)
/// 指向内核陷阱处理函数。这是陷阱处理系统的初始化函数。
///
/// ## 初始化内容
///
/// - 设置 `stvec` 寄存器指向 `trap_from_kernel`
/// - 配置直接模式 (`TrapMode::Direct`)
/// - 为内核态陷阱处理做准备
///
/// ## 调用时机
///
/// 应在系统启动早期调用，在启用中断之前完成陷阱系统初始化。
///
/// ## Note
///
/// 此函数只设置内核态陷阱入口，用户态陷阱入口会在任务切换时
/// 通过 `set_user_trap_entry()` 动态设置。
pub fn init() {
    set_kernel_trap_entry();
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

/// 设置内核态陷阱入口
///
/// 配置 `stvec` 寄存器指向内核陷阱处理函数 `trap_from_kernel`。
/// 当内核态发生陷阱时，硬件会跳转到此函数执行。
///
/// ## 安全性
///
/// 使用 `unsafe` 直接操作 CSR 寄存器，这是特权操作。
fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

/// 设置用户态陷阱入口
///
/// 配置 `stvec` 寄存器指向用户态陷阱处理入口 `TRAMPOLINE`。
/// 当用户态发生陷阱时，硬件会跳转到 Trampoline 页面执行。
///
/// ## Trampoline 机制
///
/// Trampoline 页面包含 `__alltraps` 和 `__restore` 汇编代码，
/// 负责保存和恢复完整的用户态上下文。
///
/// ## 安全性
///
/// 使用 `unsafe` 直接操作 CSR 寄存器，这是特权操作。
fn set_user_trap_entry() {
    unsafe {
        stvec::write(TRAMPOLINE as usize, TrapMode::Direct);
    }
}

/// 内核态陷阱处理函数
///
/// 当内核态发生陷阱时的处理函数。在当前的简化实现中，
/// 内核态不应该发生陷阱，因此直接触发 panic。
///
/// ## 设计原理
///
/// - 内核代码应该是可信的，不应该产生异常
/// - 如果内核态发生陷阱，说明存在严重的内核 bug
/// - 立即停止系统运行，避免进一步的损坏
///
/// ## 可能的陷阱原因
///
/// - 内核代码访问无效内存地址
/// - 内核代码执行无效指令
/// - 硬件故障或配置错误
///
/// ## 属性说明
///
/// - `#[unsafe(no_mangle)]`: 防止函数名被混淆，确保链接器能找到
/// - `-> !`: 函数永不返回，因为会触发 panic
#[unsafe(no_mangle)]
pub fn trap_from_kernel() -> ! {
    panic!("a trap {:?} from kernel!", scause::read().cause());
}

/// 陷阱处理主函数
///
/// 所有用户态陷阱的统一处理入口，根据陷阱原因分发到相应的处理逻辑。
/// 这是陷阱处理系统的核心函数，处理系统调用、异常和中断。
///
/// ## 处理流程
///
/// 1. **设置内核陷阱入口**: 防止处理过程中的嵌套陷阱
/// 2. **获取陷阱信息**: 读取 `scause` 和 `stval` 寄存器
/// 3. **分发处理**: 根据陷阱类型调用相应的处理逻辑
/// 4. **返回用户态**: 调用 `trap_return()` 恢复用户执行
///
/// ## 系统调用处理细节
///
/// 系统调用处理中有特殊的上下文管理：
/// - 首次获取陷阱上下文以读取系统调用参数
/// - 更新 `sepc` 寄存器指向下一条指令 (`pc += 4`)
/// - 调用系统调用处理函数，此时可能发生任务切换
/// - 再次获取陷阱上下文以写入返回值（因为任务切换后上下文可能变化）
///
/// ## 支持的陷阱类型
///
/// - **系统调用** (`UserEnvCall`): 处理用户程序的系统调用请求
/// - **数据访问异常** (`StoreFault`, `StorePageFault`, `LoadFault`, `LoadPageFault`): 处理数据内存访问违规
/// - **指令访问异常** (`InstructionFault`, `InstructionPageFault`): 处理指令内存访问违规
/// - **非法指令** (`IllegalInstruction`): 处理无效指令执行
/// - **时钟中断** (`SupervisorTimer`): 处理抢占式调度
///
/// ## 错误处理
///
/// - 对于致命异常（内存违规、非法指令），终止当前任务
/// - 对于未知陷阱类型，触发 panic
///
/// ## 属性说明
///
/// - `#[unsafe(no_mangle)]`: 防止函数名被混淆，汇编代码需要调用此函数
/// - `-> !`: 函数永不返回，总是通过 `trap_return()` 返回用户态
#[unsafe(no_mangle)]
pub fn trap_handler() -> ! {
    set_kernel_trap_entry();
    let scause = scause::read();
    let stval = stval::read();
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            let mut cx = current_trap_cx();
            cx.sepc += 4;
            let result = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]);
            cx = current_trap_cx();
            cx.x[10] = result as usize;
        }
        Trap::Exception(Exception::StoreFault)
        | Trap::Exception(Exception::StorePageFault)
        | Trap::Exception(Exception::LoadFault)
        | Trap::Exception(Exception::LoadPageFault)
        | Trap::Exception(Exception::InstructionFault)
        | Trap::Exception(Exception::InstructionPageFault) => {
            println!(
                "[kernel] {:?} in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.",
                scause.cause(),
                stval,
                current_trap_cx().sepc
            );
            exit_current_and_run_next(-2);
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!(
                "[kernel] IllegalInstruction in application, bad instruction = {:#x}, kernel killed it.",
                current_trap_cx().sepc
            );
            exit_current_and_run_next(-3);
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger();
            suspend_current_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval,
            );
        }
    }
    trap_return();
}

/// 陷阱返回函数
///
/// 陷阱处理完成后返回用户态的函数，负责恢复用户态执行环境并
/// 跳转回用户程序继续执行。这是陷阱处理的最后阶段。
///
/// ## 执行流程
///
/// 1. **设置用户陷阱入口**: 配置 `stvec` 指向 Trampoline
/// 2. **准备返回参数**: 获取陷阱上下文地址和用户页表标识符
/// 3. **跳转到 Trampoline**: 通过内联汇编跳转到 `__restore`
/// 4. **恢复用户状态**: `__restore` 恢复所有寄存器并执行 `sret`
///
/// ## 地址空间切换
///
/// 函数执行过程中会发生地址空间切换：
/// - 开始：内核地址空间
/// - 跳转到 Trampoline：仍在内核地址空间（Trampoline 在两个地址空间都有映射）
/// - `__restore` 执行 `sret`：切换到用户地址空间
///
/// ## Trampoline 机制
///
/// ```text
/// 地址空间切换过程:
/// ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
/// │  Kernel Space   │    │   Trampoline    │    │   User Space    │
/// │                 │───>│    (Shared)     │───>│                 │
/// │  trap_return()  │    │   __restore     │    │  User Program   │
/// └─────────────────┘    └─────────────────┘    └─────────────────┘
/// ```
///
/// ## 内联汇编
///
/// 使用内联汇编执行关键的跳转操作：
/// - `fence.i`: 指令缓存同步
/// - `jr {restore_va}`: 跳转到 `__restore` 函数
/// - 传递参数：`a0` = 陷阱上下文地址，`a1` = 用户页表标识符
///
/// ## 安全性
///
/// - 使用 `unsafe` 执行特权操作和内联汇编
/// - 地址计算确保跳转到正确的 Trampoline 位置
/// - 参数传递确保 `__restore` 获得正确的上下文信息
///
/// ## 属性说明
///
/// - `#[unsafe(no_mangle)]`: 防止函数名被混淆，可能被其他代码调用
/// - `-> !`: 函数永不返回，总是跳转到用户程序执行
#[unsafe(no_mangle)]
pub fn trap_return() -> ! {
    set_user_trap_entry();
    let trap_cx_ptr = TRAP_CONTEXT;
    let user_satp = current_user_token();
    unsafe extern "C" {
        fn __alltraps();
        fn __restore();
    }
    let restore_va = __restore as usize - __alltraps as usize + TRAMPOLINE;
    unsafe {
        asm!(
            "fence.i",
            "jr {restore_va}",
            restore_va = in(reg) restore_va,
            in("a0") trap_cx_ptr,
            in("a1") user_satp,
            options(noreturn),
        );
    }
}
