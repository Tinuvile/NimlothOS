//! # 系统调用处理模块
//!
//! 提供用户态应用程序与内核交互的系统调用接口。
//! 系统调用是用户程序请求操作系统服务的标准机制。
//!
//! ## 支持的系统调用
//!
//! - **文件系统**: [`sys_write`] - 向文件描述符写入数据
//! - **进程管理**: [`sys_exit`], [`sys_yield`], [`sys_get_time`]
//! - **内存管理**: [`sys_sbrk`] - 调整程序断点
//!
//! ## 系统调用编号
//!
//! 遵循 Linux 系统调用编号约定：
//! - `SYSCALL_WRITE` (64) - 写操作
//! - `SYSCALL_EXIT` (93) - 进程退出
//! - `SYSCALL_YIELD` (124) - 让出 CPU
//! - `SYSCALL_GET_TIME` (169) - 获取系统时间
//! - `SYSCALL_SBRK` (214) - 调整程序断点

use fs::*;
use process::*;

mod fs;
mod process;

/// 系统调用号：写操作
///
/// 对应 Linux 系统调用 `write(2)`，用于向文件描述符写入数据。
const SYSCALL_WRITE: usize = 64;

/// 系统调用号：进程退出
///
/// 对应 Linux 系统调用 `exit(2)`，用于终止当前进程。
const SYSCALL_EXIT: usize = 93;

/// 系统调用号：让出 CPU
///
/// 对应 Linux 系统调用 `sched_yield(2)`，当前任务主动让出 CPU 时间片。
const SYSCALL_YIELD: usize = 124;

/// 系统调用号：获取时间
///
/// 对应 Linux 系统调用 `gettimeofday(2)` 的简化版本，获取系统时间戳。
const SYSCALL_GET_TIME: usize = 169;

/// 系统调用号：调整程序断点
///
/// 对应 Linux 系统调用 `sbrk(2)`，用于动态调整进程的堆大小。
const SYSCALL_SBRK: usize = 214;

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
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GET_TIME => sys_get_time(),
        SYSCALL_SBRK => sys_sbrk(args[0] as i32),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
