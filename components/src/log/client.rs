//! # 日志客户端API
//!
//! 提供与原系统兼容的客户端接口和便捷宏。

use crate::log::transport::{SyscallReadFn, SyscallWriteFn};
use crate::log::{DEFAULT_LOG_FD, LogLevel, LogMessage, LogTransport, PipeTransport};
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

/// 客户端错误类型
#[derive(Debug)]
pub enum LogError {
    /// 传输错误
    TransportError,
    /// 未初始化
    NotInitialized,
    /// 消息过长
    MessageTooLong,
}

/// 客户端结果类型
pub type LogResult<T> = Result<T, LogError>;

/// 日志客户端
pub struct LogClient {
    transport: Box<dyn LogTransport + Send + Sync>,
    pid: u32,
    cpu_id: u8,
}

impl LogClient {
    /// 创建新的日志客户端
    pub fn new(transport: Box<dyn LogTransport + Send + Sync>) -> Self {
        Self {
            transport,
            pid: get_current_pid(),
            cpu_id: get_current_cpu_id(),
        }
    }

    /// 连接到默认的日志服务
    pub fn connect_with_syscalls(write_fd: usize, sys_write: SyscallWriteFn) -> LogResult<Self> {
        let transport = Box::new(PipeTransport::new_client(write_fd, sys_write));
        Ok(Self::new(transport))
    }

    /// 连接到默认的日志服务（使用默认FD）
    pub fn connect() -> LogResult<Self> {
        // 这个方法现在需要系统调用函数，在这里我们先返回错误
        Err(LogError::NotInitialized)
    }

    /// 发送日志消息
    pub fn log(&self, level: LogLevel, module: &str, message: &str) -> LogResult<()> {
        let timestamp = get_timestamp();

        let log_msg = LogMessage::new(level, self.pid, self.cpu_id, timestamp, module, message)
            .ok_or(LogError::MessageTooLong)?;

        self.transport
            .send(&log_msg)
            .map_err(|_| LogError::TransportError)
    }

    /// 记录错误日志
    pub fn error(&self, module: &str, message: &str) -> LogResult<()> {
        self.log(LogLevel::Error, module, message)
    }

    /// 记录警告日志
    pub fn warn(&self, module: &str, message: &str) -> LogResult<()> {
        self.log(LogLevel::Warn, module, message)
    }

    /// 记录信息日志
    pub fn info(&self, module: &str, message: &str) -> LogResult<()> {
        self.log(LogLevel::Info, module, message)
    }

    /// 记录调试日志
    pub fn debug(&self, module: &str, message: &str) -> LogResult<()> {
        self.log(LogLevel::Debug, module, message)
    }

    /// 记录跟踪日志
    pub fn trace(&self, module: &str, message: &str) -> LogResult<()> {
        self.log(LogLevel::Trace, module, message)
    }
}

use core::ptr;

// 全局客户端实例 - 使用原始指针避免静态可变引用问题
static mut GLOBAL_CLIENT_PTR: *const LogClient = ptr::null();
static CLIENT_INITIALIZED: AtomicU32 = AtomicU32::new(0);

/// 初始化全局日志客户端
pub fn init_log_client() -> LogResult<()> {
    if CLIENT_INITIALIZED.load(Ordering::Acquire) == 0 {
        let client = Box::new(LogClient::connect()?);
        let client_ptr = Box::into_raw(client);
        unsafe {
            GLOBAL_CLIENT_PTR = client_ptr;
        }
        CLIENT_INITIALIZED.store(1, Ordering::Release);
    }
    Ok(())
}

/// 获取全局日志客户端
pub fn get_log_client() -> LogResult<&'static LogClient> {
    if CLIENT_INITIALIZED.load(Ordering::Acquire) == 0 {
        init_log_client()?;
    }

    unsafe {
        let ptr = GLOBAL_CLIENT_PTR;
        if ptr.is_null() {
            Err(LogError::NotInitialized)
        } else {
            Ok(&*ptr)
        }
    }
}

/// 便捷的日志记录函数
pub fn log_message(level: LogLevel, module: &str, message: &str) {
    if let Ok(client) = get_log_client() {
        let _ = client.log(level, module, message);
    }
}

// 便捷宏定义（与原系统保持兼容）

/// 错误级别日志宏
#[macro_export]
macro_rules! error {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::log::log_message(
            $crate::log::LogLevel::Error,
            module_path!(),
            &alloc::format!($fmt $(, $($arg)+)?)
        )
    };
}

/// 警告级别日志宏
#[macro_export]
macro_rules! warn {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::log::log_message(
            $crate::log::LogLevel::Warn,
            module_path!(),
            &alloc::format!($fmt $(, $($arg)+)?)
        )
    };
}

/// 信息级别日志宏
#[macro_export]
macro_rules! info {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::log::log_message(
            $crate::log::LogLevel::Info,
            module_path!(),
            &alloc::format!($fmt $(, $($arg)+)?)
        )
    };
}

/// 调试级别日志宏
#[macro_export]
macro_rules! debug {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::log::log_message(
            $crate::log::LogLevel::Debug,
            module_path!(),
            &alloc::format!($fmt $(, $($arg)+)?)
        )
    };
}

/// 跟踪级别日志宏
#[macro_export]
macro_rules! trace {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::log::log_message(
            $crate::log::LogLevel::Trace,
            module_path!(),
            &alloc::format!($fmt $(, $($arg)+)?)
        )
    };
}

// 辅助函数 - 获取系统信息
fn get_current_pid() -> u32 {
    // 这里需要调用系统调用获取当前进程ID
    // 暂时返回固定值，实际需要调用 sys_pid()
    0
}

fn get_current_cpu_id() -> u8 {
    // 这里需要获取当前CPU ID
    // 暂时返回固定值
    0
}

fn get_timestamp() -> u32 {
    // 这里需要获取系统时间戳
    // 暂时使用静态计数器，实际需要调用 sys_time()
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

// 系统调用声明（实际集成时需要）
unsafe extern "C" {
    fn sys_pid() -> isize;
    fn sys_time() -> isize;
}

// 重新导出便捷宏
pub use crate::{debug, error, info, trace, warn};
