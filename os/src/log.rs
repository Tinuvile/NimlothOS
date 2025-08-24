//! # 日志系统模块
//!
//! 提供结构化的日志输出功能，支持多种日志级别和彩色输出。
//! 实现了标准的 Rust `log` crate 接口，提供丰富的调试和诊断信息。
//!
//! ## 功能特性
//!
//! - **多级别日志**: 支持 ERROR、WARN、INFO、DEBUG、TRACE 五个级别
//! - **彩色输出**: 不同级别使用不同颜色，提高可读性
//! - **详细信息**: 包含时间戳、CPU ID、线程 ID、模块名、文件位置
//! - **环境配置**: 通过环境变量 `LOG` 控制日志级别
//! - **便捷宏**: 提供 `error!`、`warn!`、`info!`、`debug!`、`trace!` 宏
//!
//! ## 日志格式
//!
//! ```text
//! LEVEL [T0001] [CPU0] [TH0] [module::name] [file.rs:42] message
//! ```
//!
//! ## 颜色方案
//!
//! - 🔴 **ERROR**: 红色 (31)
//! - 🟡 **WARN**: 亮黄色 (93)  
//! - 🔵 **INFO**: 蓝色 (34)
//! - 🟢 **DEBUG**: 绿色 (32)
//! - ⚫ **TRACE**: 暗灰色 (90)

use log::{self, Level, LevelFilter, Log, Metadata, Record};

use crate::println;

/// 简单日志实现
///
/// 实现标准的 `Log` trait，提供基本的日志功能。
/// 支持按级别过滤和格式化输出。
struct SimpleLogger;

impl Log for SimpleLogger {
    /// 检查是否应该记录指定级别的日志
    ///
    /// ## Arguments
    ///
    /// * `metadata` - 日志元数据，包含级别、目标模块等信息
    ///
    /// ## Returns
    ///
    /// 返回 `true` 表示应该记录该日志，`false` 表示过滤掉
    ///
    /// ## Implementation
    ///
    /// 当前实现始终返回 `true`，实际的级别过滤由 `log` crate 处理。
    /// 注释掉的代码展示了如何实现自定义过滤逻辑。
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // 发布版本：只处理 Info 及以上级别的日志
        // metadata.level() <= Level::Info

        // 调试版本：记录所有级别的日志
        true
    }

    /// 记录一条日志
    ///
    /// 格式化日志消息并输出到控制台，包含时间戳、CPU 信息、
    /// 模块名、文件位置等详细信息。
    ///
    /// ## Arguments
    ///
    /// * `record` - 日志记录，包含级别、消息、位置等信息
    ///
    /// ## 输出格式
    ///
    /// 包含位置信息的格式：
    /// ```text
    /// LEVEL [T0001] [CPU0] [TH0] [module] [file.rs:line] message
    /// ```
    ///
    /// 不包含位置信息的格式：
    /// ```text
    /// LEVEL [T0001] [CPU0] [TH0] [module] [unknown] message
    /// ```
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // 根据日志级别选择颜色
            let color = match record.level() {
                Level::Error => 31, // 红色
                Level::Warn => 93,  // 亮黄色
                Level::Info => 34,  // 蓝色
                Level::Debug => 32, // 绿色
                Level::Trace => 90, // 暗灰色
            };

            // 收集上下文信息
            let timestamp = timestamp();
            let cpu_id = cpu_id();
            let thread_id = thread_id();
            let module = record.target();

            if let (Some(file), Some(line)) = (record.file(), record.line()) {
                // 提取文件名（去掉路径）
                let file_name = file.split('/').last().unwrap_or(file);
                println!(
                    "\u{1B}[{}m{:>5} [T{:>4}] [CPU{}] [TH{}] [{}] [{}:{}] {}\u{1B}[0m",
                    color,
                    record.level(),
                    timestamp,
                    cpu_id,
                    thread_id,
                    module,
                    file_name,
                    line,
                    record.args()
                );
            } else {
                // 没有位置信息的情况
                println!(
                    "\u{1B}[{}m{:>5} [T{:>4}] [CPU{}] [TH{}] [{}] [unknown] {}\u{1B}[0m",
                    color,
                    record.level(),
                    timestamp,
                    cpu_id,
                    thread_id,
                    module,
                    record.args()
                );
            }
        }
    }

    /// 刷新日志输出缓冲区
    ///
    /// 当前实现为空，因为控制台输出是同步的，不需要显式刷新。
    fn flush(&self) {}
}

/// 全局时间戳计数器
///
/// 简单的递增计数器，用于为每条日志生成唯一的时间戳。
/// 在多线程环境中可能存在竞争条件，但对于调试目的足够。
static mut TICK_COUNT: usize = 0;

/// 获取时间戳
///
/// 返回一个单调递增的时间戳，用于标识日志的顺序。
///
/// ## Returns
///
/// 返回当前的时间戳值
///
/// ## Safety
///
/// 使用 `unsafe` 代码访问全局可变变量，在单线程环境下是安全的。
fn timestamp() -> usize {
    unsafe {
        TICK_COUNT += 1;
        TICK_COUNT
    }
}

/// 获取 CPU ID
///
/// 读取 `mhartid` CSR 寄存器获取当前 CPU 核心的 ID。
///
/// ## Returns
///
/// 返回当前 CPU 核心的硬件线程 ID
///
/// ## Safety
///
/// 使用内联汇编读取 CSR 寄存器，这是一个特权操作。
fn cpu_id() -> usize {
    unsafe {
        let cpu_id: usize;
        core::arch::asm!("csrr {}, mhartid", out(reg) cpu_id, options(nomem, nostack));
        cpu_id
    }
}

/// 获取线程 ID
///
/// 目前与 CPU ID 相同，读取 `mhartid` CSR 寄存器。
/// 在真正的多线程实现中，这应该返回线程的唯一标识符。
///
/// ## Returns
///
/// 返回线程 ID（当前等同于 CPU ID）
///
/// ## Note
///
/// 这是一个临时实现，真正的线程系统需要维护独立的线程 ID。
fn thread_id() -> usize {
    unsafe {
        let thread_id: usize;
        core::arch::asm!("csrr {}, mhartid", out(reg) thread_id, options(nomem, nostack));
        thread_id
    }
}

/// 初始化日志系统
///
/// 设置全局日志记录器并配置日志级别。日志级别可以通过编译时
/// 环境变量 `LOG` 进行配置。
///
/// ## 环境变量配置
///
/// - `LOG=ERROR` - 只输出错误级别日志
/// - `LOG=WARN` - 输出警告及以上级别日志
/// - `LOG=INFO` - 输出信息及以上级别日志（默认）
/// - `LOG=DEBUG` - 输出调试及以上级别日志  
/// - `LOG=TRACE` - 输出所有级别日志
///
/// ## Usage
///
/// ```rust
/// fn main() {
///     log::init();  // 初始化日志系统
///     info!("System started");  // 现在可以使用日志宏
/// }
/// ```
///
/// ## Note
///
/// 必须在使用任何日志宏之前调用此函数，通常在系统初始化早期调用。
pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;

    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(match option_env!("LOG") {
        Some("ERROR") => LevelFilter::Error,
        Some("WARN") => LevelFilter::Warn,
        Some("INFO") => LevelFilter::Info,
        Some("DEBUG") => LevelFilter::Debug,
        Some("TRACE") => LevelFilter::Trace,
        _ => LevelFilter::Info,
    });
}

/// 错误级别日志宏
///
/// 输出红色的错误消息，用于记录系统错误和异常情况。
///
/// ## Usage
///
/// ```rust
/// error!("Failed to load application {}", app_id);
/// error!("Memory allocation failed: {}", error_msg);
/// ```
#[macro_export]
macro_rules! error {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[31mERROR {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

/// 警告级别日志宏
///
/// 输出亮黄色的警告消息，用于记录潜在问题和异常情况。
///
/// ## Usage
///
/// ```rust
/// warn!("Task {} is taking too long", task_id);
/// warn!("Low memory warning: {} bytes remaining", free_memory);
/// ```
#[macro_export]
macro_rules! warn {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[93m WARN {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

/// 信息级别日志宏
///
/// 输出蓝色的信息消息，用于记录重要的系统事件和状态变化。
///
/// ## Usage
///
/// ```rust
/// info!("System initialized successfully");
/// info!("Task {} completed in {} ms", task_id, duration);
/// ```
#[macro_export]
macro_rules! info {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[34m INFO {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

/// 调试级别日志宏
///
/// 输出绿色的调试消息，用于开发和调试过程中的详细信息输出。
///
/// ## Usage
///
/// ```rust
/// debug!("Entering function with parameter: {}", param);
/// debug!("Variable state: x={}, y={}", x, y);
/// ```
#[macro_export]
macro_rules! debug {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[32mDEBUG {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

/// 跟踪级别日志宏
///
/// 输出暗灰色的跟踪消息，用于最详细的执行流程跟踪。
///
/// ## Usage
///
/// ```rust
/// trace!("Function entry: process_request()");
/// trace!("Loop iteration {}: value={}", i, value);
/// ```
#[macro_export]
macro_rules! trace {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[90mTRACE {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}
