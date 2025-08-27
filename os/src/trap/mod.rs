//! # 陷阱和中断处理模块
//!
//! 处理所有从用户态到内核态的陷阱 (trap)，包括系统调用、异常和中断。
//! 当前实现将“同步异常转信号”的语义融入到陷阱处理流程中：对内存访问/非法指令
//! 等异常不再直接打印并杀死进程，而是先投递相应信号，随后统一在信号处理阶段
//! 执行默认/用户自定义动作。
//!
//! ## 处理的陷阱类型
//!
//! - **系统调用** (`UserEnvCall`): 用户程序请求内核服务
//! - **时钟中断** (`SupervisorTimer`): 实现抢占式多进程调度
//! - **数据访问异常** (`StoreFault`, `StorePageFault`, `LoadFault`, `LoadPageFault`): 数据内存访问违规
//! - **指令访问异常** (`InstructionFault`, `InstructionPageFault`): 指令内存访问违规
//! - **非法指令** (`IllegalInstruction`): 执行无效指令
//!
//! ## 执行流程（更新）
//!
//! 1. **陷阱触发**: 用户态程序执行 `ecall` 或发生异常/中断
//! 2. **硬件切换**: CPU 自动切换到 S 模式，跳转到 `stvec` 指定的处理程序
//! 3. **上下文保存**: `__alltraps` 保存所有寄存器到陷阱上下文
//! 4. **处理分发**: `trap_handler` 根据陷阱类型执行相应处理（见下）
//! 5. **信号阶段**: 调用 `handle_signals()` 检查/进入用户信号处理；
//!    对致命信号，`check_signals_error_of_current()` 会返回标准退出码并退出进程
//! 6. **上下文恢复**: `trap_return()` → `__restore` 恢复寄存器并返回用户态
//!
//! ## 寄存器使用
//!
//! - `stvec`: 陷阱向量寄存器，指向陷阱处理入口 `__alltraps`
//! - `scause`: 陷阱原因寄存器，标识陷阱类型和具体原因
//! - `stval`: 陷阱值寄存器，包含相关的地址或值信息
//! - `sstatus`: 状态寄存器，控制中断使能和特权级
//! - `sepc`: 异常程序计数器，指向触发陷阱的指令地址

use crate::config::{TRAMPOLINE, TRAP_CONTEXT};
use crate::process::{
    SignalFlags, check_signals_error_of_current, current_add_signal, current_process,
    current_trap_cx, current_user_token, exit_current_and_run_next, handle_signals,
    take_current_process,
};
use crate::process::{add_process_with_priority, get_time_slice};
use crate::syscall::syscall;
use crate::timer::next_trigger;
use crate::{println, process::suspend_current_and_run_next};
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
/// 此函数只设置内核态陷阱入口，用户态陷阱入口会在进程切换时
/// 通过 `set_user_trap_entry()` 动态设置。
pub fn init() {
    set_kernel_trap_entry();
    enable_timer_interrupt();
}

/// 启用时钟中断
///
/// 在监督者中断使能寄存器 (`sie`) 中启用时钟中断位，
/// 允许时钟中断触发陷阱进入内核进行进程调度。
///
/// ## 功能
///
/// - 设置 `sie.STIE` 位，启用监督者时钟中断
/// - 配合时钟设备实现抢占式多进程调度
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

#[unsafe(no_mangle)]
pub fn trap_from_kernel() -> ! {
    use riscv::register::sepc;
    println!("stval = {:#x}, sepc = {:#x}", stval::read(), sepc::read());
    panic!("a trap {:?} from kernel!", scause::read().cause());
}

/// 陷阱处理主函数
///
/// 用户态陷阱统一入口：
/// - 系统调用：两次获取/写回 Trap 上下文（因期间可能发生进程切换）
/// - 访问/执行异常：不直接终止，改为投递 `SIGSEGV`/`SIGILL` 等信号
/// - 时钟中断：设置下一次触发并让出 CPU
/// 处理完毕后进入信号阶段，必要时退出当前进程，然后返回用户态继续执行。
///
/// ## 处理流程
///
/// 1. **设置内核陷阱入口**: 防止处理过程中的嵌套陷阱（`stvec` 指向内核）
/// 2. **获取陷阱信息**: 读取 `scause` 和 `stval` 寄存器
/// 3. **分发处理**: 根据陷阱类型执行：系统调用/异常转信号/时钟中断让出
/// 4. **信号处理**: `handle_signals()` 进入/完成用户处理；对致命信号退出
/// 5. **返回用户态**: `trap_return()` 恢复用户执行
///
/// ## 系统调用处理细节
///
/// 系统调用处理中有特殊的上下文管理：
/// - 首次获取陷阱上下文以读取参数
/// - 提前移动 `sepc += 4` 跳过 `ecall`
/// - 调用系统调用处理函数（期间可能调度）
/// - 再次获取陷阱上下文写回返回值（避免因调度导致的上下文失配）
///
/// ## 支持的陷阱类型
///
/// - **系统调用** (`UserEnvCall`): 处理用户程序的系统调用请求
/// - **数据访问异常** (`StoreFault`, `StorePageFault`, `LoadFault`, `LoadPageFault`): 处理数据内存访问违规
/// - **指令访问异常** (`InstructionFault`, `InstructionPageFault`): 处理指令内存访问违规
/// - **非法指令** (`IllegalInstruction`): 处理无效指令执行
/// - **时钟中断** (`SupervisorTimer`): 处理抢占式调度
///
/// ## 错误/信号处理
///
/// - 访问/执行异常：改为投递信号，由信号阶段决定是否终止或进入用户处理
/// - 未知陷阱类型：触发 panic
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
            // println!(
            //     "[kernel] {:?} in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.",
            //     scause.cause(),
            //     stval,
            //     current_trap_cx().sepc
            // );
            current_add_signal(SignalFlags::SIGSEGV);
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            // println!(
            //     "[kernel] IllegalInstruction in application, bad instruction = {:#x}, kernel killed it.",
            //     current_trap_cx().sepc
            // );
            current_add_signal(SignalFlags::SIGILL);
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            next_trigger();

            // MLFQ 时间片降级逻辑
            if let Some(process) = current_process() {
                let mut inner = process.inner_exclusive_access();
                inner.time_slice_used += 1;

                // 检查是否用完时间片
                if inner.time_slice_used >= inner.time_slice_limit {
                    // 时间片用完，需要降级
                    let current_priority = inner.priority;
                    use crate::config::MLFQ_QUEUE_COUNT;
                    let new_priority = if current_priority < MLFQ_QUEUE_COUNT - 1 {
                        current_priority + 1
                    } else {
                        current_priority
                    };

                    // 更新进程的优先级信息
                    inner.priority = new_priority;
                    inner.time_slice_used = 0;
                    inner.time_slice_limit = get_time_slice(new_priority);

                    drop(inner);

                    // 取出当前进程并重新加入对应优先级队列
                    if let Some(process) = take_current_process() {
                        add_process_with_priority(process, new_priority);
                    }
                }
            }

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

    handle_signals();

    if let Some((errno, msg)) = check_signals_error_of_current() {
        println!("[kernel] {}", msg);
        exit_current_and_run_next(errno);
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
