//! # 日志服务器实现
//!
//! 独立的日志服务进程，接收并处理来自其他进程的日志消息。

use crate::log::{LogMessage, LogTransport, PipeTransport};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// 日志服务器错误类型
#[derive(Debug)]
pub enum ServerError {
    /// 传输错误
    TransportError,
    /// 初始化失败
    InitializationFailed,
}

/// 服务器结果类型
pub type ServerResult<T> = Result<T, ServerError>;

/// 日志服务器
pub struct LogServer {
    transport: Box<dyn LogTransport>,
    running: bool,
}

impl LogServer {
    /// 创建新的日志服务器
    pub fn new(read_fd: usize) -> Self {
        let transport = Box::new(PipeTransport::new_server(read_fd));
        Self {
            transport,
            running: false,
        }
    }

    /// 启动服务器
    pub fn start(&mut self) -> ServerResult<()> {
        self.running = true;
        self.run_loop()
    }

    /// 停止服务器
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// 主运行循环
    fn run_loop(&mut self) -> ServerResult<()> {
        while self.running {
            match self.process_messages() {
                Ok(_) => continue,
                Err(ServerError::TransportError) => {
                    // 传输错误，可能是没有消息，继续运行
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// 处理消息
    fn process_messages(&mut self) -> ServerResult<()> {
        // 批量处理消息以提高效率
        let mut messages = Vec::new();

        // 尝试读取多条消息
        for _ in 0..10 {
            // 最多一次处理10条消息
            match self.transport.try_receive() {
                Ok(Some(msg)) => messages.push(msg),
                Ok(None) => break, // 没有更多消息
                Err(_) => break,   // 读取错误
            }
        }

        // 处理收集到的消息
        for message in messages {
            self.handle_message(message);
        }

        Ok(())
    }

    /// 处理单条消息
    fn handle_message(&self, message: LogMessage) {
        // 格式化并输出消息
        let formatted = message.format();
        self.output_message(&formatted);
    }

    /// 输出消息到控制台
    fn output_message(&self, message: &str) {
        // 这里需要调用系统的输出函数
        // 暂时什么都不做，实际需要调用 sys_write 到标准输出

        // 实际实现应该是：
        // unsafe {
        //     let bytes = message.as_bytes();
        //     sys_write(1, bytes.as_ptr(), bytes.len()); // 写到标准输出
        // }

        // 为了演示，我们暂时将消息存储起来（实际不应该这样）
        // 在真实环境中，这里应该直接输出到控制台
        let _ = message; // 避免未使用警告
    }

    /// 批量处理模式
    pub fn process_batch(&mut self, max_messages: usize) -> ServerResult<usize> {
        let mut processed = 0;

        while processed < max_messages && self.running {
            match self.transport.try_receive() {
                Ok(Some(message)) => {
                    self.handle_message(message);
                    processed += 1;
                }
                Ok(None) => break, // 没有更多消息
                Err(_) => break,   // 错误
            }
        }

        Ok(processed)
    }

    /// 检查是否还在运行
    pub fn is_running(&self) -> bool {
        self.running
    }
}

// 服务器主函数 - 在独立进程中运行
pub fn run_log_server(read_fd: usize) -> ! {
    let mut server = LogServer::new(read_fd);

    // 启动服务器
    match server.start() {
        Ok(_) => {
            // 正常退出
        }
        Err(_) => {
            // 错误退出
        }
    }

    // 服务器进程退出
    unsafe {
        sys_exit(0);
    }
}

// 创建日志服务进程的辅助函数
pub fn spawn_log_service() -> ServerResult<usize> {
    // 创建管道
    let mut pipe_fd = [0usize; 2];
    unsafe {
        if sys_pipe(pipe_fd.as_mut_ptr()) < 0 {
            return Err(ServerError::InitializationFailed);
        }
    }

    let read_fd = pipe_fd[0];
    let write_fd = pipe_fd[1];

    // 创建子进程
    unsafe {
        let pid = sys_fork();
        if pid == 0 {
            // 子进程：日志服务器
            sys_close(write_fd); // 关闭写端
            run_log_server(read_fd); // 运行服务器（不返回）
        } else if pid > 0 {
            // 父进程：内核
            sys_close(read_fd); // 关闭读端
            return Ok(write_fd); // 返回写端给客户端使用
        } else {
            return Err(ServerError::InitializationFailed);
        }
    }
}

// 系统调用声明
extern "C" {
    fn sys_pipe(pipe_fd: *mut usize) -> isize;
    fn sys_fork() -> isize;
    fn sys_close(fd: usize) -> isize;
    fn sys_exit(exit_code: i32) -> !;
    fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize;
}
