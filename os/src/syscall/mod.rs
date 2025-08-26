//! # 系统调用处理模块
//!
//! 提供用户态应用程序与内核交互的系统调用接口。
//! 系统调用是用户程序请求操作系统服务的标准机制。
//!
//! ## 支持的系统调用
//!
//! - **文件系统**:
//!   - [`sys_open`]     - 打开文件
//!   - [`sys_close`]    - 关闭文件
//!   - [`sys_dup`]    - 复制文件描述符
//!   - [`sys_pipe`]    - 创建管道
//!   - [`sys_read`]  - 从文件描述符读取数据
//!   - [`sys_write`] - 向文件描述符写入数据
//! - **进程管理**:
//!   - [`sys_exit`]     - 进程退出
//!   - [`sys_yield`]    - 让出 CPU
//!   - [`sys_time`] - 获取系统时间
//!   - [`sys_pid`]   - 获取当前进程 PID
//!   - [`sys_fork`]     - 创建子进程（复制地址空间）
//!   - [`sys_exec`]     - 替换为新程序镜像
//!   - [`sys_waitpid`]  - 等待子进程结束并获取退出码
//!   - [`sys_kill`]     - 发送信号给进程
//!   - [`sys_sigaction`] - 设置信号处理
//!   - [`sys_sigprocmask`] - 设置信号掩码
//!   - [`sys_sigreturn`] - 从信号处理返回
//!
//! ## 系统调用编号
//!
//! 遵循 Linux 系统调用编号约定：
//! - `SYSCALL_OPEN` (56)         - 打开文件
//! - `SYSCALL_CLOSE` (57)        - 关闭文件
//! - `SYSCALL_READ` (63)         - 读操作
//! - `SYSCALL_WRITE` (64)        - 写操作
//! - `SYSCALL_EXIT` (93)         - 进程退出
//! - `SYSCALL_YIELD` (124)       - 让出 CPU
//! - `SYSCALL_TIME` (169)        - 获取系统时间
//! - `SYSCALL_PID` (172)         - 获取进程 PID
//! - `SYSCALL_FORK` (220)        - 创建子进程
//! - `SYSCALL_EXEC` (221)        - 执行新程序
//! - `SYSCALL_WAITPID` (260)     - 等待子进程
//! - `SYSCALL_DUP` (24)          - 复制文件描述符
//! - `SYSCALL_PIPE` (59)         - 创建管道
//! - `SYSCALL_KILL` (129)        - 发送信号给进程
//! - `SYSCALL_SIGACTION` (134)   - 设置信号处理
//! - `SYSCALL_SIGPROCMASK` (135) - 设置信号掩码
//! - `SYSCALL_SIGRETURN` (139)   - 从信号处理返回

use crate::process::SignalAction;
use fs::*;
use process::*;

mod fs;
mod process;

pub use fs::*;
pub use process::*;

const SYSCALL_DUP: usize = 24;
const SYSCALL_OPEN: usize = 56;
const SYSCALL_CLOSE: usize = 57;
const SYSCALL_PIPE: usize = 59;
const SYSCALL_READ: usize = 63;
const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_YIELD: usize = 124;
const SYSCALL_KILL: usize = 129;
const SYSCALL_SIGACTION: usize = 134;
const SYSCALL_SIGPROCMASK: usize = 135;
const SYSCALL_SIGRETURN: usize = 139;
const SYSCALL_TIME: usize = 169;
const SYSCALL_PID: usize = 172;
const SYSCALL_FORK: usize = 220;
const SYSCALL_EXEC: usize = 221;
const SYSCALL_WAITPID: usize = 260;

/// 系统调用分发器
///
/// 这是系统调用处理的主入口点，负责根据系统调用号分发到具体的处理函数。
/// 该函数由陷阱处理器调用，将用户态的系统调用请求转换为内核函数调用。
///
/// ## Arguments
///
/// * `syscall_id` - 系统调用编号，标识要执行的系统调用类型
/// * `args` - 系统调用参数数组，最多支持 3 个参数
///
/// ## Returns
///
/// 返回系统调用的执行结果：
/// - 成功时返回非负值（具体含义取决于系统调用类型）
/// - 失败时返回负值（错误码）
///
/// ## Panics
///
/// 当遇到不支持的系统调用编号时会触发 panic
///
/// ## 调用约定
///
/// 遵循 RISC-V 系统调用约定：
/// - `a7` 寄存器存放系统调用号 (`syscall_id`)
/// - `a0`, `a1`, `a2` 寄存器存放参数 (`args[0]`, `args[1]`, `args[2]`)
/// - `a0` 寄存器存放返回值
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    match syscall_id {
        SYSCALL_READ => sys_read(args[0], args[1] as *const u8, args[2]),
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_TIME => sys_time(),
        SYSCALL_PID => sys_pid(),
        SYSCALL_FORK => sys_fork(),
        SYSCALL_EXEC => sys_exec(args[0] as *const u8, args[1] as *const usize),
        SYSCALL_WAITPID => sys_waitpid(args[0] as isize, args[1] as *mut i32),
        SYSCALL_OPEN => sys_open(args[0] as *const u8, args[1] as u32),
        SYSCALL_CLOSE => sys_close(args[0]),
        SYSCALL_DUP => sys_dup(args[0]),
        SYSCALL_PIPE => sys_pipe(args[0] as *mut usize),
        SYSCALL_KILL => sys_kill(args[0], args[1] as i32),
        SYSCALL_SIGACTION => sys_sigaction(
            args[0] as i32,
            args[1] as *const SignalAction,
            args[2] as *mut SignalAction,
        ),
        SYSCALL_SIGPROCMASK => sys_sigprocmask(args[0] as u32),
        SYSCALL_SIGRETURN => sys_sigreturn(),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
