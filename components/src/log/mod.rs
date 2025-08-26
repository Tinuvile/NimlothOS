//! # 简化微内核日志系统
//!
//! 提供与原有日志系统兼容的接口，但使用微内核架构实现。
//! 通过IPC与独立的日志服务进程通信。

mod client;
mod message;
mod transport;

#[cfg(feature = "server")]
mod server;

// 公开接口
pub use client::{LogClient, LogError, LogResult, get_log_client, init_log_client, log_message};
pub use message::{LogLevel, LogMessage};
pub use transport::{LogTransport, PipeTransport, SyscallReadFn, SyscallWriteFn};

#[cfg(feature = "server")]
pub use server::LogServer;

// 便捷宏
pub use client::{debug, error, info, trace, warn};

/// 默认日志管道文件描述符
pub const DEFAULT_LOG_FD: usize = 3;

/// 最大日志消息长度
pub const MAX_MESSAGE_LEN: usize = 512;

/// 最大模块名长度
pub const MAX_MODULE_LEN: usize = 32;
