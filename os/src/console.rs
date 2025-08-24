//! # 控制台输出模块
//!
//! 提供格式化文本输出功能，实现类似标准库的 `print!` 和 `println!` 宏。
//! 通过 SBI 接口与底层硬件交互，将文本输出到控制台。
//!
//! ## 功能特性
//!
//! - **格式化输出**: 支持 Rust 标准的格式化字符串语法
//! - **SBI 集成**: 通过 SBI 调用实现跨平台控制台输出
//! - **宏接口**: 提供便捷的 `print!` 和 `println!` 宏
//! - **UTF-8 支持**: 完全支持 Unicode 字符输出
//!
//! ## 使用示例
//!
//! ```rust
//! println!("Hello, world!");
//! print!("Answer: {}", 42);
//! println!("Debug info: {:?}", some_struct);
//! ```

use crate::sbi::console_putchar;
use core::fmt::{self, Write};

/// 标准输出结构体
///
/// 实现了 `Write` trait，将格式化的文本通过 SBI 接口输出到控制台。
/// 这是一个零大小类型 (ZST)，不占用内存空间。
struct Stdout;

impl Write for Stdout {
    /// 将字符串写入标准输出
    ///
    /// 遍历字符串中的每个字符，通过 SBI 的 `console_putchar` 函数
    /// 逐个输出到控制台。
    ///
    /// ## Arguments
    ///
    /// * `s` - 要输出的字符串切片
    ///
    /// ## Returns
    ///
    /// 总是返回 `Ok(())`，因为 SBI 调用不会失败
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            console_putchar(c as usize);
        }
        Ok(())
    }
}

/// 格式化输出函数
///
/// 接受格式化参数并输出到控制台，是 `print!` 和 `println!` 宏的底层实现。
///
/// ## Arguments
///
/// * `args` - 格式化参数，由 `format_args!` 宏生成
///
/// ## Panics
///
/// 如果格式化或写入过程中发生错误会 panic，但在正常情况下不会发生
pub fn print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

/// 格式化打印宏（不换行）
///
/// 类似于标准库的 `print!` 宏，将格式化的文本输出到控制台，
/// 不会在末尾添加换行符。
///
/// ## 语法
///
/// ```rust
/// print!("format string", arg1, arg2, ...);
/// ```
///
/// ## 格式化支持
///
/// 支持 Rust 标准的格式化语法：
/// - `{}` - 默认格式化
/// - `{:?}` - Debug 格式化
/// - `{:x}` - 十六进制格式化
/// - `{:>10}` - 右对齐，宽度为10
///
/// ## Examples
///
/// ```rust
/// print!("Hello, ");
/// print!("world!");        // 输出: Hello, world!
/// print!("Value: {}", 42); // 输出: Value: 42
/// ```
#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?));
    }
}

/// 格式化打印宏（带换行）
///
/// 类似于标准库的 `println!` 宏，将格式化的文本输出到控制台，
/// 并在末尾自动添加换行符。
///
/// ## 语法
///
/// ```rust
/// println!("format string", arg1, arg2, ...);
/// ```
///
/// ## 与 print! 的区别
///
/// - `print!` - 不添加换行符
/// - `println!` - 自动添加换行符 (`\n`)
///
/// ## Examples
///
/// ```rust
/// println!("Hello, world!")           // 输出: Hello, world!
/// println!("Answer: {}", 42)          // 输出: Answer: 42
/// println!("Debug: {:?}", some_value) // 输出: Debug: SomeValue
/// ```
///
/// ## Implementation
///
/// 内部通过字符串连接将 `\n` 添加到格式字符串末尾，
/// 然后调用 [`print!`] 宏进行实际输出。
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?))
    }
}
