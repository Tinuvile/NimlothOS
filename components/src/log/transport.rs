//! # 日志传输层
//!
//! 提供基于管道的IPC传输机制。

use crate::log::message::LogMessage;
use alloc::vec::Vec;

/// 传输层错误类型
#[derive(Debug)]
pub enum TransportError {
    /// 写入失败
    WriteFailed,
    /// 读取失败
    ReadFailed,
    /// 管道未连接
    NotConnected,
}

/// 传输层结果类型
pub type TransportResult<T> = Result<T, TransportError>;

/// 日志传输抽象接口
pub trait LogTransport {
    /// 发送消息
    fn send(&self, message: &LogMessage) -> TransportResult<()>;
    /// 接收消息
    fn receive(&self) -> TransportResult<LogMessage>;
    /// 尝试非阻塞接收
    fn try_receive(&self) -> TransportResult<Option<LogMessage>>;
}

/// 系统调用函数类型
pub type SyscallWriteFn = fn(fd: usize, buf: &[u8]) -> isize;
pub type SyscallReadFn = fn(fd: usize, buf: &mut [u8]) -> isize;

/// 基于管道的传输实现
pub struct PipeTransport {
    /// 写入文件描述符（客户端使用）
    write_fd: Option<usize>,
    /// 读取文件描述符（服务器使用）
    read_fd: Option<usize>,
    /// 系统调用函数指针
    sys_write: Option<SyscallWriteFn>,
    sys_read: Option<SyscallReadFn>,
}

impl PipeTransport {
    /// 创建新的管道传输（客户端模式）
    pub fn new_client(write_fd: usize, sys_write: SyscallWriteFn) -> Self {
        Self {
            write_fd: Some(write_fd),
            read_fd: None,
            sys_write: Some(sys_write),
            sys_read: None,
        }
    }

    /// 创建新的管道传输（服务器模式）
    pub fn new_server(read_fd: usize, sys_read: SyscallReadFn) -> Self {
        Self {
            write_fd: None,
            read_fd: Some(read_fd),
            sys_write: None,
            sys_read: Some(sys_read),
        }
    }

    /// 写入数据到管道
    fn write_to_pipe(&self, fd: usize, data: &[u8]) -> TransportResult<usize> {
        if let Some(sys_write) = self.sys_write {
            let written = sys_write(fd, data);
            if written > 0 {
                Ok(written as usize)
            } else {
                Err(TransportError::WriteFailed)
            }
        } else {
            Err(TransportError::NotConnected)
        }
    }

    /// 从管道读取数据
    fn read_from_pipe(&self, fd: usize, buffer: &mut [u8]) -> TransportResult<usize> {
        if let Some(sys_read) = self.sys_read {
            let read_size = sys_read(fd, buffer);
            if read_size > 0 {
                Ok(read_size as usize)
            } else if read_size == 0 {
                Ok(0) // EOF
            } else {
                Err(TransportError::ReadFailed)
            }
        } else {
            Err(TransportError::NotConnected)
        }
    }
}

impl LogTransport for PipeTransport {
    fn send(&self, message: &LogMessage) -> TransportResult<()> {
        let fd = self.write_fd.ok_or(TransportError::NotConnected)?;
        let data = message.serialize();

        // 先发送消息长度
        let len_bytes = (data.len() as u32).to_le_bytes();
        self.write_to_pipe(fd, &len_bytes)?;

        // 再发送消息数据
        self.write_to_pipe(fd, &data)?;

        Ok(())
    }

    fn receive(&self) -> TransportResult<LogMessage> {
        let fd = self.read_fd.ok_or(TransportError::NotConnected)?;

        // 先读取消息长度
        let mut len_buf = [0u8; 4];
        self.read_from_pipe(fd, &mut len_buf)?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;

        // 读取消息数据
        let mut msg_buf = Vec::with_capacity(msg_len);
        msg_buf.resize(msg_len, 0);
        self.read_from_pipe(fd, &mut msg_buf)?;

        // 反序列化消息
        LogMessage::deserialize(&msg_buf).ok_or(TransportError::ReadFailed)
    }

    fn try_receive(&self) -> TransportResult<Option<LogMessage>> {
        // 简化实现 - 实际需要非阻塞读取
        match self.receive() {
            Ok(msg) => Ok(Some(msg)),
            Err(TransportError::ReadFailed) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

// 为了与系统调用交互，我们需要定义系统调用接口
// 这些函数将在实际集成时实现
unsafe extern "C" {
    fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize;
    fn sys_read(fd: usize, buf: *mut u8, len: usize) -> isize;
}

// 实际的系统调用包装函数
#[allow(dead_code)]
fn syscall_write(fd: usize, data: &[u8]) -> isize {
    unsafe { sys_write(fd, data.as_ptr(), data.len()) }
}

#[allow(dead_code)]
fn syscall_read(fd: usize, buffer: &mut [u8]) -> isize {
    unsafe { sys_read(fd, buffer.as_mut_ptr(), buffer.len()) }
}
