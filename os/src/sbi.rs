#![allow(unused)]
//! # SBI (Supervisor Binary Interface) 封装
//!
//! 基于 `sbi-rt` 提供与 RISC-V SBI 固件的交互接口。该模块对常用功能
//! 进行轻量封装，屏蔽底层调用细节，便于在内核中直接使用。
//!
//! ## 提供能力
//! - 控制台 I/O：[`console_putchar`], [`console_getchar`]
//! - 定时器：[`set_timer`]
//! - 系统复位：[`shutdown`]
//!
//! ## 实现说明
//! - 使用 `sbi_rt::legacy::*` 访问 legacy 扩展（兼容广泛）
//! - `console_getchar` 的“无字符可读”返回值由固件实现决定（见函数文档）
//! - 定时器使用绝对时间（timebase 计数）

/// 控制台输出单个字符
///
/// 通过 SBI legacy 控制台接口输出一个字符。此调用为同步输出，
/// 直至固件接受该字符。
///
/// ## Arguments
/// * `c` - 要输出的字符（ASCII 码）
pub fn console_putchar(c: usize) {
    #[allow(deprecated)]
    sbi_rt::legacy::console_putchar(c);
}

/// 控制台读取单个字符（非阻塞）
///
/// 从 SBI legacy 控制台尝试读取一个字符。
/// - 有字符可读：返回其 ASCII 码
/// - 无字符可读：返回值依赖固件实现，常见有两种：
///   - 返回 `0`（本工程在 QEMU 环境中采用该语义）
///   - 返回 `usize::MAX`（源自 legacy 规范中 `-1` 的无符号表示）
///
/// 建议调用方对两种情况都做兼容处理。
pub fn console_getchar() -> usize {
    #[allow(deprecated)]
    sbi_rt::legacy::console_getchar()
}

/// 设置时钟中断触发时间（绝对时间）
///
/// 通过 SBI `set_timer` 配置下一次时钟中断触发的绝对时刻。
/// 触发时刻以 timebase 计数为单位。
///
/// ## Arguments
/// * `timer` - 触发时间（绝对计数值）
pub fn set_timer(timer: usize) {
    sbi_rt::set_timer(timer as _);
}

/// 关闭（或复位）系统
///
/// 使用 SBI System Reset 扩展请求系统关闭。根据 `failure` 参数选择
/// 关闭原因码，以便固件或上层记录关机原因。
///
/// ## Arguments
/// * `failure` - 是否因为系统故障而关闭
///   - `false`：`NoReason`
///   - `true`：`SystemFailure`
///
/// ## Behavior
/// - 正常情况下，此函数不会返回。
pub fn shutdown(failure: bool) -> ! {
    use sbi_rt::{NoReason, Shutdown, SystemFailure, system_reset};
    if !failure {
        system_reset(Shutdown, NoReason);
    } else {
        system_reset(Shutdown, SystemFailure);
    }
    unreachable!()
}
