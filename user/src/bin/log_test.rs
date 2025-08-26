#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate user_lib;

use alloc::format;
use components::log::{LogClient, LogLevel};
use user_lib::{close, exit, fork, pipe, read, waitpid, write};

// 包装系统调用函数以匹配components的接口
fn syscall_write_wrapper(fd: usize, buf: &[u8]) -> isize {
    write(fd, buf)
}

fn syscall_read_wrapper(fd: usize, buf: &mut [u8]) -> isize {
    read(fd, buf)
}

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("=== log test ===");

    // 创建管道
    let mut pipe_fd = [0usize; 2];
    if pipe(&mut pipe_fd) < 0 {
        println!("Failed to create pipe");
        return -1;
    }

    let read_fd = pipe_fd[0];
    let write_fd = pipe_fd[1];

    println!("pipe created: read_fd={}, write_fd={}", read_fd, write_fd);

    let pid = fork();
    if pid == 0 {
        // 子进程：模拟日志服务器
        close(write_fd); // 关闭写端

        println!("[log server] start, waiting for messages...");

        // 简单的日志服务器：读取并显示消息
        loop {
            let mut buffer = [0u8; 256];
            let bytes_read = syscall_read_wrapper(read_fd, &mut buffer);

            if bytes_read > 0 {
                // 尝试反序列化日志消息
                if let Some(log_msg) =
                    components::log::LogMessage::deserialize(&buffer[..bytes_read as usize])
                {
                    // 使用格式化输出
                    println!("[log server] received message: {}", log_msg.format());
                } else {
                    println!("[log server] received raw data: {} bytes", bytes_read);
                }
            } else if bytes_read == 0 {
                println!("[log server] pipe closed, exit");
                break;
            } else {
                // 读取错误，可能是非阻塞读取没有数据
                continue;
            }
        }

        close(read_fd);
        exit(0);
    } else if pid > 0 {
        // 父进程：客户端
        close(read_fd); // 关闭读端

        println!("[client] create log client...");

        // 创建日志客户端
        match LogClient::connect_with_syscalls(write_fd, syscall_write_wrapper) {
            Ok(client) => {
                println!("[client] log client created");

                // 发送一些测试日志
                let _ = client.error("test_module", "This is an error message");
                let _ = client.warn("test_module", "This is a warning message");
                let _ = client.info("test_module", "This is an info message");
                let _ = client.debug("test_module", "This is a debug message");
                let _ = client.trace("test_module", "This is a trace message");

                println!("[client] all test messages sent");
            }
            Err(_) => {
                println!("[client] create log client failed");
            }
        }

        close(write_fd); // 关闭写端，通知服务器结束

        // 等待子进程结束
        let mut exit_code = 0;
        waitpid(pid as usize, &mut exit_code);
        println!("[client] log server exited, exit code: {}", exit_code);

        println!("=== log test done ===");
    } else {
        println!("Fork failed");
        return -1;
    }

    0
}
