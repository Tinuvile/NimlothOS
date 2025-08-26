//! # 简化日志消息格式
//!
//! 定义微内核日志系统中使用的简单消息格式。

use crate::log::{MAX_MESSAGE_LEN, MAX_MODULE_LEN};
use alloc::string::{String, ToString};
use core::fmt;

/// 日志级别定义（与原系统保持一致）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

/// 简化的日志消息结构
///
/// 消息格式：
/// ```text
/// ┌─────────┬─────────┬─────────┬─────────┬─────────────┬─────────────┐
/// │ Level   │ PID     │ CPU     │ Time    │   Module    │   Message   │
/// │ (1B)    │ (4B)    │ (1B)    │ (4B)    │  Variable   │  Variable   │
/// └─────────┴─────────┴─────────┴─────────┴─────────────┴─────────────┘
/// ```
#[derive(Debug, Clone)]
pub struct LogMessage {
    /// 日志级别
    pub level: LogLevel,
    /// 进程ID
    pub pid: u32,
    /// CPU ID
    pub cpu_id: u8,
    /// 时间戳
    pub timestamp: u32,
    /// 模块名
    pub module: String,
    /// 消息内容
    pub message: String,
}

impl LogLevel {
    /// 获取日志级别名称
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }

    /// 获取日志级别对应的ANSI颜色代码
    pub fn color_code(&self) -> u8 {
        match self {
            LogLevel::Error => 31, // 红色
            LogLevel::Warn => 93,  // 亮黄色
            LogLevel::Info => 34,  // 蓝色
            LogLevel::Debug => 32, // 绿色
            LogLevel::Trace => 90, // 暗灰色
        }
    }
}

impl LogMessage {
    /// 创建新的日志消息
    pub fn new(
        level: LogLevel,
        pid: u32,
        cpu_id: u8,
        timestamp: u32,
        module: &str,
        message: &str,
    ) -> Option<Self> {
        // 验证长度限制
        if module.len() > MAX_MODULE_LEN || message.len() > MAX_MESSAGE_LEN {
            return None;
        }

        Some(Self {
            level,
            pid,
            cpu_id,
            timestamp,
            module: module.to_string(),
            message: message.to_string(),
        })
    }

    /// 序列化为字节流（简单版本）
    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buffer = alloc::vec::Vec::new();

        // 写入基本信息
        buffer.push(self.level as u8);
        buffer.extend_from_slice(&self.pid.to_le_bytes());
        buffer.push(self.cpu_id);
        buffer.extend_from_slice(&self.timestamp.to_le_bytes());

        // 写入长度信息
        buffer.push(self.module.len() as u8);
        buffer.extend_from_slice(&(self.message.len() as u16).to_le_bytes());

        // 写入字符串内容
        buffer.extend_from_slice(self.module.as_bytes());
        buffer.extend_from_slice(self.message.as_bytes());

        buffer
    }

    /// 从字节流反序列化
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            // 最小头部大小
            return None;
        }

        let mut offset = 0;

        // 读取基本信息
        let level = match data[offset] {
            1 => LogLevel::Error,
            2 => LogLevel::Warn,
            3 => LogLevel::Info,
            4 => LogLevel::Debug,
            5 => LogLevel::Trace,
            _ => return None,
        };
        offset += 1;

        let pid = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        let cpu_id = data[offset];
        offset += 1;

        let timestamp = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        // 读取长度信息
        if offset + 3 > data.len() {
            return None;
        }

        let module_len = data[offset] as usize;
        offset += 1;

        let message_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        // 读取字符串内容
        if offset + module_len + message_len > data.len() {
            return None;
        }

        let module = String::from_utf8_lossy(&data[offset..offset + module_len]).to_string();
        offset += module_len;

        let message = String::from_utf8_lossy(&data[offset..offset + message_len]).to_string();

        Some(Self {
            level,
            pid,
            cpu_id,
            timestamp,
            module,
            message,
        })
    }

    /// 格式化输出（与原系统保持一致的格式）
    pub fn format(&self) -> String {
        alloc::format!(
            "\u{1B}[{}m{:>5} [T{:>4}] [CPU{}] [{}] {}\u{1B}[0m",
            self.level.color_code(),
            self.level.as_str(),
            self.timestamp,
            self.cpu_id,
            self.module,
            self.message
        )
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl fmt::Display for LogMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}
