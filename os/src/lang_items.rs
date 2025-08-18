//! # 语言项实现模块
//!
//! 实现 Rust 语言运行时所需的核心语言项 (lang items)，主要是 panic 处理机制。
//! 在 `no_std` 环境中，必须手动实现这些语言项以支持 Rust 的基本运行时功能。
//!
//! ## 实现的语言项
//!
//! - **`#[panic_handler]`**: panic 处理器，定义程序 panic 时的行为
//!
//! ## Panic 处理策略
//!
//! 1. **信息输出**: 显示 panic 发生的位置和原因
//! 2. **系统关闭**: 通过 SBI 接口安全关闭系统
//! 3. **栈追踪**: (可选) 打印函数调用栈信息
//!
//! ## 设计原则
//!
//! - **信息完整性**: 尽可能提供详细的错误信息
//! - **系统稳定性**: panic 后安全关闭，避免系统处于不一致状态
//! - **调试友好性**: 提供足够信息便于问题定位

#[allow(unused_imports)]
use crate::stack_trace::print_stack_trace;
use crate::{println, sbi::shutdown};
use core::panic::PanicInfo;

/// Panic 处理器
///
/// 当程序发生 panic 时，Rust 运行时会调用此函数处理异常情况。
/// 该函数会输出 panic 信息，然后安全关闭系统。
///
/// ## 处理流程
///
/// 1. **提取位置信息**: 尝试获取 panic 发生的文件名、行号等
/// 2. **输出错误信息**: 将 panic 消息和位置信息打印到控制台
/// 3. **栈追踪**: (可选) 打印函数调用栈
/// 4. **系统关闭**: 通过 SBI 接口安全关闭系统
///
/// ## Arguments
///
/// * `info` - Panic 信息结构，包含错误消息和发生位置
///
/// ## Panic 信息格式
///
/// - **有位置信息**: `Paniced at file.rs:line:column: message`
/// - **无位置信息**: `Paniced: message`
///
/// ## Examples
///
/// ```rust
/// panic!("Something went wrong!");
/// // 输出: Paniced at src/main.rs:42:5: Something went wrong!
///
/// assert_eq!(1, 2);
/// // 输出: Paniced at src/main.rs:45:5: assertion failed: `(left == right)`
/// ```
///
/// ## Safety
///
/// 这是一个发散函数 (`!`)，调用后永不返回。系统会被安全关闭，
/// 确保不会留下不一致的状态。
///
/// ## Note
///
/// - 栈追踪功能当前被注释掉，可在需要时启用
/// - 该函数必须标记为 `#[panic_handler]` 以被 Rust 运行时识别
/// - 在整个程序中只能有一个 panic 处理器
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "Paniced at {}:{}:{}: {}",
            location.file(),
            location.line(),
            location.column(),
            info.message()
        );
    } else {
        println!("Paniced: {}", info.message());
    }

    // 可选：打印栈追踪信息（当前已禁用）
    // unsafe { print_stack_trace() };

    // 安全关闭系统
    shutdown();
}
