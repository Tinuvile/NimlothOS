//! # 文件系统相关系统调用
//!
//! 实现与文件 I/O 相关的系统调用，提供文件读写、打开关闭等基本文件操作功能。
//! 支持标准输入输出设备访问和文件系统操作。
//!
//! ## 支持的系统调用
//!
//! - [`sys_write`]   - 向文件描述符写入数据
//! - [`sys_read`]    - 从文件描述符读取数据  
//! - [`sys_open`]    - 打开文件并返回文件描述符
//! - [`sys_close`]   - 关闭文件描述符
//! - [`sys_dup`]     - 复制文件描述符
//! - [`sys_pipe`]    - 创建管道
//!
//! ## 文件描述符管理
//!
//! 每个进程维护一个文件描述符表，用于跟踪打开的文件：
//! - 标准输入 (fd=0)
//! - 标准输出 (fd=1)
//! - 标准错误 (fd=2)
//! - 用户打开的文件 (fd>=3)
//!
//! ## 地址空间转换
//!
//! 所有系统调用都通过 [`translated_byte_buffer`] 和 [`translated_str`]
//! 安全地访问用户空间数据，确保地址空间隔离。

use crate::fs::{OpenFlags, make_pipe, open_file};
use crate::mm::{UserBuffer, translated_byte_buffer, translated_refmut, translated_str};
use crate::process::{current_process, current_user_token};
use alloc::sync::Arc;

/// 系统调用：向文件描述符写入数据
///
/// 实现 `write(2)` 系统调用，向指定的文件描述符写入数据。
/// 支持向标准输出、文件等可写设备写入数据。
///
/// ## Arguments
///
/// * `fd` - 文件描述符，指定要写入的目标
/// * `buf` - 指向用户空间缓冲区的指针，包含要写入的数据
/// * `len` - 要写入的字节数
///
/// ## Returns
///
/// - 成功时返回实际写入的字节数
/// - 失败时返回 -1
///
/// ## 错误情况
///
/// - 文件描述符无效或超出范围
/// - 文件描述符未打开
/// - 文件不支持写入操作
///
/// ## 安全考虑
///
/// 通过 [`translated_byte_buffer`] 安全地访问用户空间缓冲区，
/// 确保地址空间隔离和内存安全。
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let process = current_process().unwrap();
    let inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

/// 系统调用：从文件描述符读取数据
///
/// 实现 `read(2)` 系统调用，从指定的文件描述符读取数据到用户缓冲区。
/// 支持从标准输入、文件等可读设备读取数据。
///
/// ## Arguments
///
/// * `fd` - 文件描述符，指定要读取的源
/// * `buf` - 指向用户空间缓冲区的指针，用于存储读取的数据
/// * `len` - 要读取的最大字节数
///
/// ## Returns
///
/// - 成功时返回实际读取的字节数
/// - 到达文件末尾时返回 0
/// - 失败时返回 -1
///
/// ## 错误情况
///
/// - 文件描述符无效或超出范围
/// - 文件描述符未打开
/// - 文件不支持读取操作
///
/// ## 安全考虑
///
/// 通过 [`translated_byte_buffer`] 安全地访问用户空间缓冲区，
/// 确保地址空间隔离和内存安全。
pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let process = current_process().unwrap();
    let inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.readable() {
            return -1;
        }
        let file = file.clone();
        drop(inner);
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

/// 系统调用：打开文件
///
/// 实现 `open(2)` 系统调用，打开指定路径的文件并返回文件描述符。
/// 支持不同的打开模式，如只读、只写、读写等。
///
/// ## Arguments
///
/// * `path` - 指向用户空间以 `\0` 结尾的文件路径字符串
/// * `flags` - 打开标志位，定义文件的打开模式
///
/// ## 支持的标志位
///
/// - `O_RDONLY` (0) - 只读模式
/// - `O_WRONLY` (1) - 只写模式
/// - `O_RDWR` (2) - 读写模式
/// - `O_CREAT` (64) - 如果文件不存在则创建
/// - `O_TRUNC` (512) - 如果文件存在则截断
///
/// ## Returns
///
/// - 成功时返回新分配的文件描述符（非负整数）
/// - 失败时返回 -1
///
/// ## 错误情况
///
/// - 文件路径无效或不存在
/// - 权限不足
/// - 文件描述符表已满
///
/// ## 安全考虑
///
/// 通过 [`translated_str`] 安全地读取用户空间的文件路径字符串。
pub fn sys_open(path: *const u8, flags: u32) -> isize {
    let process = current_process().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = process.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

/// 系统调用：关闭文件描述符
///
/// 实现 `close(2)` 系统调用，关闭指定的文件描述符并释放相关资源。
/// 关闭后该文件描述符可以被重新分配。
///
/// ## Arguments
///
/// * `fd` - 要关闭的文件描述符
///
/// ## Returns
///
/// - 成功时返回 0
/// - 失败时返回 -1
///
/// ## 错误情况
///
/// - 文件描述符无效或超出范围
/// - 文件描述符已经关闭
///
/// ## 资源管理
///
/// 关闭文件描述符会：
/// - 从进程的文件描述符表中移除该条目
/// - 释放相关的文件对象引用
/// - 使该文件描述符号可以被后续的 `open` 调用重用
pub fn sys_close(fd: usize) -> isize {
    let process = current_process().unwrap();
    let mut inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// 系统调用：复制文件描述符（dup）
///
/// 实现 `dup(2)` 的核心语义：为已打开的文件描述符分配一个新的最小可用
/// 文件描述符编号，并让两者引用同一个底层文件对象（共享偏移与状态）。
///
/// ## Arguments
///
/// * `fd` - 需要复制的已有文件描述符
///
/// ## Returns
///
/// - 成功：返回新的文件描述符编号
/// - 失败：返回 -1（如 `fd` 无效或未打开）
///
/// ## 共享语义
///
/// - 新旧两个 fd 指向同一 `File` 对象，读写偏移共享
/// - 关闭任意一个 fd 不影响另一个 fd 的有效性（引用计数减少）
pub fn sys_dup(fd: usize) -> isize {
    let process = current_process().unwrap();
    let mut inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    new_fd as isize
}

/// 系统调用：创建管道
///
/// 创建一对相互连接的文件描述符：`pipe[0]` 为读端、`pipe[1]` 为写端。
/// 进程或父子进程间可通过该字节流进行单向通信。实现遵循 POSIX 语义：
/// - 读端在缓冲区空且写端全部关闭时返回 0（EOF）
/// - 写端在缓冲区满时阻塞（当前实现通过让出 CPU 实现）
///
/// ## Arguments
///
/// * `pipe` - 指向用户空间 usize[2] 的指针，用于写回读/写端 fd
///
/// ## Returns
///
/// - 成功返回 0，失败返回 -1
///
/// ## 安全考虑
///
/// - 使用 `translated_refmut` 将两个 fd 写回到用户空间
/// - fd 的实际分配来源于当前进程的 fd 表
pub fn sys_pipe(pipe: *mut usize) -> isize {
    let process = current_process().unwrap();
    let token = current_user_token();
    let mut inner = process.inner_exclusive_access();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    *translated_refmut(token, pipe) = read_fd;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}
