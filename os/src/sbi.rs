//! # SBI (Supervisor Binary Interface) 调用模块
//!
//! 提供与 RISC-V SBI 固件的交互接口，实现操作系统内核与底层固件之间的通信。
//! SBI 是 RISC-V 架构中监督者模式软件与机器模式固件之间的标准接口。
//!
//! ## 支持的 SBI 扩展
//!
//! ### Legacy 扩展 (0.1 版本)
//! - 控制台输入输出
//! - 时钟设置
//! - IPI (处理器间中断)
//! - TLB 管理
//! - 系统关闭
//!
//! ### 系统复位扩展 (SRST)
//! - 系统关闭和重启功能
//! - 支持不同的复位类型和原因
//!
//! ## 调用约定
//!
//! SBI 调用使用 `ecall` 指令从 S 模式陷入 M 模式：
//! - `a7` 寄存器：扩展 ID (EID)
//! - `a6` 寄存器：函数 ID (FID)  
//! - `a0-a2` 寄存器：函数参数
//! - 返回值通过 `a0` (错误码) 和 `a1` (返回值) 寄存器传递

/// SBI Legacy 扩展函数 ID

/// 设置时钟中断触发时间
const SBI_SET_TIMER: usize = 0;

/// 控制台字符输出
const SBI_CONSOLE_PUTCHAR: usize = 1;

/// 控制台字符输入 (未使用)
#[allow(dead_code)]
const SBI_CONSOLE_GETCHAR: usize = 2;

/// 清除处理器间中断 (未使用)
#[allow(dead_code)]
const SBI_CLEAR_IPI: usize = 3;

/// 发送处理器间中断 (未使用)
#[allow(dead_code)]
const SBI_SEND_IPI: usize = 4;

/// 远程指令缓存刷新 (未使用)
#[allow(dead_code)]
const SBI_REMOTE_FENCE_I: usize = 5;

/// 远程地址空间刷新 (未使用)
#[allow(dead_code)]
const SBI_REMOTE_SFENCE_VMA: usize = 6;

/// 带 ASID 的远程地址空间刷新 (未使用)
#[allow(dead_code)]
const SBI_REMOTE_SFENCE_VMA_ASID: usize = 7;

/// 系统关闭 (Legacy)
#[allow(dead_code)]
const SBI_SHUTDOWN: usize = 8;

/// SBI 调用返回值结构
///
/// 封装 SBI 调用的返回信息，包含错误码和实际返回值。
/// 遵循 SBI 规范的返回值约定。
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct SbiRet {
    /// 错误码，0 表示成功，负值表示错误
    pub error: usize,
    /// 实际返回值，具体含义取决于调用的 SBI 函数
    pub value: usize,
}

/// 系统复位扩展 ID (SRST)
///
/// SRST 扩展提供标准化的系统复位功能，支持关闭和重启。
/// 扩展 ID 为 ASCII "SRST" 的数值表示。
const SRST_EXTENSION: usize = 0x53525354;

/// 系统复位函数 ID
const SBI_SYSTEM_RESET: usize = 0;

/// 系统复位类型枚举
///
/// 定义不同类型的系统复位操作，用于指示固件执行何种复位行为。
#[repr(usize)]
#[allow(dead_code)]
enum SystemResetType {
    /// 正常关闭系统
    Shutdown = 0,
    /// 冷重启 - 完全断电重启
    ColdReboot = 1,
    /// 热重启 - 不断电重启
    WarmReboot = 2,
}

/// 系统复位原因枚举
///
/// 说明执行系统复位的原因，用于日志记录和故障分析。
#[repr(usize)]
#[allow(dead_code)]
enum SystemResetReason {
    /// 无特定原因
    NoReason = 0,
    /// 系统故障导致的复位
    SystemFailure = 1,
}

/// 执行 SBI 调用
///
/// 这是所有 SBI 接口的底层实现，通过 `ecall` 指令从监督者模式
/// 陷入机器模式，调用 SBI 固件提供的服务。
///
/// ## Arguments
///
/// * `eid` - 扩展 ID，标识要调用的 SBI 扩展
/// * `fid` - 函数 ID，标识扩展内的具体函数
/// * `arg0` - 第一个参数
/// * `arg1` - 第二个参数  
/// * `arg2` - 第三个参数
///
/// ## Returns
///
/// 返回 [`SbiRet`] 结构，包含错误码和返回值
///
/// ## Safety
///
/// 使用内联汇编执行 `ecall` 指令，假设 SBI 固件正确实现。
///
/// ## 寄存器约定
///
/// - `a7` (x17): 扩展 ID
/// - `a6` (x16): 函数 ID
/// - `a0-a2` (x10-x12): 函数参数
/// - 返回：`a0` 为错误码，`a1` 为返回值
#[inline(always)]
fn sbi_call(eid: usize, fid: usize, arg0: usize, arg1: usize, arg2: usize) -> SbiRet {
    let (error, value);
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") arg0 => error,
            inlateout("a1") arg1 => value,
            in("a2") arg2,
            in("a6") fid,
            in("a7") eid,
            options(nostack, preserves_flags)
        );
    }
    SbiRet { error, value }
}

/// 控制台字符输出
///
/// 通过 SBI 接口向控制台输出单个字符，是控制台输出的基础函数。
///
/// ## Arguments
///
/// * `c` - 要输出的字符，以 `usize` 形式传递（实际为 ASCII 码值）
///
/// ## Usage
///
/// ```rust
/// console_putchar('H' as usize);  // 输出字符 'H'
/// console_putchar(65);            // 输出字符 'A' (ASCII 65)
/// ```
///
/// ## Note
///
/// - 该函数是同步的，会阻塞直到字符输出完成
/// - 不处理换行符的特殊转换
/// - 是 `print!` 和 `println!` 宏的底层实现
pub fn console_putchar(c: usize) {
    sbi_call(SBI_CONSOLE_PUTCHAR, 0, c, 0, 0);
}

/// 系统关闭
///
/// 通过 SBI 系统复位扩展安全关闭系统。该函数会请求固件关闭系统，
/// 正常情况下不会返回。
///
/// ## 关闭流程
///
/// 1. 调用 SRST 扩展的系统复位函数
/// 2. 指定复位类型为关闭 (`Shutdown`)
/// 3. 指定复位原因为无特定原因 (`NoReason`)
/// 4. 固件执行关闭操作
///
/// ## Returns
///
/// 该函数的返回类型为 `!`（never type），表示正常情况下不会返回。
/// 如果 SBI 调用失败或固件不支持关闭功能，会触发 panic。
///
/// ## Panics
///
/// 如果系统未能正常关闭（SBI 调用返回），会触发 panic 以确保系统不会
/// 处于未定义状态。
///
/// ## Usage
///
/// ```rust
/// // 在 panic 处理程序中关闭系统
/// shutdown(); // 此后不会执行到任何代码
/// ```
pub fn shutdown() -> ! {
    let _ = sbi_call(
        SRST_EXTENSION,
        SBI_SYSTEM_RESET,
        SystemResetType::Shutdown as usize,
        SystemResetReason::NoReason as usize,
        0,
    );
    panic!("It should have shutdown !")
}

/// 设置时钟中断触发时间
///
/// 通过 SBI 接口配置时钟中断在指定时间触发，用于实现抢占式任务调度。
/// 时间值基于系统的时钟周期计数。
///
/// ## Arguments
///
/// * `time` - 时钟中断触发的绝对时间，单位为时钟周期
///
/// ## 工作原理
///
/// 1. SBI 固件会将传入的时间值与当前时钟进行比较
/// 2. 当系统时钟达到指定时间时，触发时钟中断
/// 3. 中断会导致 CPU 跳转到中断处理程序
///
/// ## Usage
///
/// ```rust
/// let current_time = get_time();
/// let next_trigger = current_time + CLOCK_FREQ / 100;  // 10ms 后触发
/// set_timer(next_trigger);
/// ```
///
/// ## Note
///
/// - 时间值必须是绝对时间，不是相对时间
/// - 如果指定的时间已经过去，中断会立即触发
/// - 每次处理时钟中断后都需要重新设置下次触发时间
pub fn set_timer(time: usize) {
    sbi_call(SBI_SET_TIMER, 0, time, 0, 0);
}
