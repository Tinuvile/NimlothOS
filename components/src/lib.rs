#![no_std]

//! # NimlothOS 组件系统
//!
//! 提供操作系统各种组件的模块化实现，包括微内核日志系统等。

extern crate alloc;

pub mod easy_fs;
pub mod log;

// 重新导出常用类型
pub use log::{LogClient, LogError, LogLevel, LogResult};

#[cfg(feature = "server")]
pub use log::LogServer;

// 便捷宏将在client.rs中定义，这里不重复导出
