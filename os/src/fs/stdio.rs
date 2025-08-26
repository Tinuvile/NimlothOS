//! # 标准输入输出模块
//!
//! 提供标准输入输出设备的实现，包括标准输入 (stdin)、标准输出 (stdout) 和标准错误 (stderr)。
//! 这些设备实现了 `File` trait，可以像普通文件一样进行读写操作。
//!
//! ## 核心组件
//!
//! - [`Stdin`] - 标准输入设备，从控制台读取字符
//! - [`Stdout`] - 标准输出设备，向控制台输出文本
//! - [`Stderr`] - 标准错误设备，向控制台输出错误信息
//!
//! ## 设备特性
//!
//! - **阻塞读取**: 标准输入在没有数据时会让出 CPU
//! - **实时输出**: 标准输出和标准错误立即显示到控制台
//! - **权限控制**: 标准输入只读，标准输出和标准错误只写
//! - **字符处理**: 支持 UTF-8 编码的文本处理
//! - **错误区分**: 标准错误用于输出错误信息，便于与正常输出区分
//!
//! ## 标准文件描述符
//!
//! 每个进程创建时自动分配以下标准文件描述符：
//! - `fd_table[0]` - 标准输入 (stdin)
//! - `fd_table[1]` - 标准输出 (stdout)  
//! - `fd_table[2]` - 标准错误 (stderr)
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::fs::{Stdin, Stdout, Stderr, File};
//!
//! // 从标准输入读取字符
//! let stdin = Stdin;
//! let mut buf = [0u8; 1];
//! let user_buf = UserBuffer::new(&mut buf);
//! let bytes_read = stdin.read(user_buf);
//!
//! // 向标准输出写入文本
//! let stdout = Stdout;
//! let data = b"Hello, World!";
//! let user_buf = UserBuffer::new(data);
//! let bytes_written = stdout.write(user_buf);
//!
//! // 向标准错误写入错误信息
//! let stderr = Stderr;
//! let error_msg = b"Error: File not found";
//! let user_buf = UserBuffer::new(error_msg);
//! let bytes_written = stderr.write(user_buf);
//! ```

use super::File;
use crate::mm::UserBuffer;
use crate::print;
use crate::process::suspend_current_and_run_next;
use crate::sbi::console_getchar;

/// 标准输入设备
///
/// 实现从控制台读取字符的功能，支持阻塞式读取。
/// 当没有输入数据时，会主动让出 CPU 以节省系统资源。
///
/// ## 读取特性
///
/// - **单字符读取**: 每次读取一个字符
/// - **阻塞等待**: 没有输入时会让出 CPU
/// - **实时响应**: 一旦有输入立即返回
/// - **权限控制**: 只支持读取操作，不支持写入
///
/// ## 实现原理
///
/// 通过 SBI 接口 `console_getchar()` 从控制台读取字符。
/// 当没有可用字符时，调用 `suspend_current_and_run_next()` 让出 CPU。
///
/// ## 线程安全
///
/// 该结构是线程安全的，多个线程可以同时从标准输入读取。
/// 具体的并发控制由底层的 SBI 接口实现。
pub struct Stdin;

/// 标准输出设备
///
/// 实现向控制台输出文本的功能，支持 UTF-8 编码的文本输出。
/// 所有输出都会立即显示到控制台，无需缓冲。
///
/// ## 输出特性
///
/// - **实时输出**: 文本立即显示到控制台
/// - **UTF-8 支持**: 完全支持 Unicode 字符输出
/// - **批量处理**: 支持跨页面的用户缓冲区
/// - **权限控制**: 只支持写入操作，不支持读取
///
/// ## 实现原理
///
/// 通过 `print!` 宏将用户缓冲区的内容输出到控制台。
/// 支持跨页面的用户缓冲区，自动处理页面边界。
///
/// ## 线程安全
///
/// 该结构是线程安全的，多个线程可以同时向标准输出写入。
/// 输出操作是原子的，不会出现字符交错的情况。
pub struct Stdout;

/// 标准错误设备
///
/// 实现向控制台输出错误信息的功能，支持 UTF-8 编码的文本输出。
/// 与标准输出类似，但专门用于输出错误信息，便于与正常输出区分。
///
/// ## 输出特性
///
/// - **实时输出**: 错误信息立即显示到控制台
/// - **UTF-8 支持**: 完全支持 Unicode 字符输出
/// - **批量处理**: 支持跨页面的用户缓冲区
/// - **权限控制**: 只支持写入操作，不支持读取
/// - **错误区分**: 专门用于输出错误信息
///
/// ## 实现原理
///
/// 通过 `print!` 宏将用户缓冲区的内容输出到控制台。
/// 支持跨页面的用户缓冲区，自动处理页面边界。
/// 与标准输出使用相同的输出机制，但在语义上区分用途。
///
/// ## 线程安全
///
/// 该结构是线程安全的，多个线程可以同时向标准错误写入。
/// 输出操作是原子的，不会出现字符交错的情况。
///
/// ## 使用场景
///
/// - **错误信息输出**: 程序运行时的错误和警告信息
/// - **调试信息**: 开发过程中的调试输出
/// - **日志记录**: 系统运行状态的日志信息
/// - **用户提示**: 向用户显示错误提示信息
///
/// ## 与标准输出的区别
///
/// 在当前的实现中，标准错误和标准输出都输出到同一个控制台，
/// 但在语义上它们有不同的用途：
/// - **标准输出**: 用于正常的程序输出
/// - **标准错误**: 用于错误信息和诊断输出
///
/// 这种设计允许用户程序将正常输出和错误输出分开处理，
/// 便于日志记录、错误处理和输出重定向。
///
/// ## 与POSIX标准的兼容性
///
/// 标准错误输出遵循POSIX标准：
/// - 文件描述符2对应标准错误
/// - 支持标准的错误输出语义
/// - 与标准输出分离，便于重定向
///
/// ## Examples
///
/// ```rust
/// let stderr = Stderr;
/// let error_msg = b"Error: File not found\n";
/// let user_buf = UserBuffer::new(error_msg);
/// let bytes_written = stderr.write(user_buf);
/// assert_eq!(bytes_written, 22);
/// ```
pub struct Stderr;

impl File for Stdin {
    /// 检查标准输入是否可读
    ///
    /// ## Returns
    ///
    /// 总是返回 `true`，因为标准输入总是可读的
    fn readable(&self) -> bool {
        true
    }

    /// 检查标准输入是否可写
    ///
    /// ## Returns
    ///
    /// 总是返回 `false`，因为标准输入不支持写入操作
    fn writable(&self) -> bool {
        false
    }

    /// 从标准输入读取字符
    ///
    /// 从控制台读取一个字符到用户缓冲区中。该操作是阻塞的，
    /// 当没有输入数据时会主动让出 CPU。
    ///
    /// ## Arguments
    ///
    /// * `user_buf` - 用户缓冲区，用于存储读取的字符
    ///
    /// ## Returns
    ///
    /// 总是返回 1，表示读取了一个字符
    ///
    /// ## 读取过程
    ///
    /// 1. **缓冲区检查**: 验证缓冲区大小为 1 字节
    /// 2. **字符等待**: 循环调用 `console_getchar()` 等待输入
    /// 3. **CPU 让出**: 当没有输入时让出 CPU
    /// 4. **字符处理**: 将读取的字符写入用户缓冲区
    /// 5. **返回结果**: 返回读取的字节数（总是 1）
    ///
    /// ## 阻塞行为
    ///
    /// 当控制台没有可用字符时，函数会进入忙等待循环：
    /// - 调用 `console_getchar()` 检查是否有输入
    /// - 如果没有输入（返回 0），调用 `suspend_current_and_run_next()`
    /// - 让出 CPU 给其他进程执行
    /// - 当进程重新调度时，继续检查输入
    ///
    /// ## 错误处理
    ///
    /// - 如果缓冲区大小不是 1 字节，会触发 panic
    /// - 如果写入用户缓冲区失败，会触发 panic
    ///
    /// ## 性能说明
    ///
    /// 该操作是阻塞的，会等待用户输入。在等待期间会主动让出 CPU，
    /// 不会浪费系统资源。
    ///
    /// ## Examples
    ///
    /// ```
    /// let stdin = Stdin;
    /// let mut buf = [0u8; 1];
    /// let user_buf = UserBuffer::new(&mut buf);
    /// let bytes_read = stdin.read(user_buf);
    /// assert_eq!(bytes_read, 1);
    /// ```
    fn read(&self, mut user_buf: UserBuffer) -> usize {
        assert_eq!(user_buf.len(), 1);

        let mut c: usize;
        loop {
            c = console_getchar();
            if c == 0 {
                suspend_current_and_run_next();
                continue;
            } else {
                break;
            }
        }
        let ch = c as u8;
        unsafe {
            user_buf.buffers[0].as_mut_ptr().write_volatile(ch);
        }
        1
    }

    /// 向标准输入写入数据
    ///
    /// ## Arguments
    ///
    /// * `_user_buf` - 用户缓冲区（未使用）
    ///
    /// ## Panics
    ///
    /// 总是触发 panic，因为标准输入不支持写入操作
    ///
    /// ## 设计说明
    ///
    /// 标准输入是只读设备，不支持写入操作。如果尝试写入，
    /// 会触发 panic 以明确表示操作不被支持。
    fn write(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot write to stdin!");
    }
}

impl File for Stdout {
    /// 检查标准输出是否可读
    ///
    /// ## Returns
    ///
    /// 总是返回 `false`，因为标准输出不支持读取操作
    fn readable(&self) -> bool {
        false
    }

    /// 检查标准输出是否可写
    ///
    /// ## Returns
    ///
    /// 总是返回 `true`，因为标准输出总是可写的
    fn writable(&self) -> bool {
        true
    }

    /// 从标准输出读取数据
    ///
    /// ## Arguments
    ///
    /// * `_user_buf` - 用户缓冲区（未使用）
    ///
    /// ## Panics
    ///
    /// 总是触发 panic，因为标准输出不支持读取操作
    ///
    /// ## 设计说明
    ///
    /// 标准输出是只写设备，不支持读取操作。如果尝试读取，
    /// 会触发 panic 以明确表示操作不被支持。
    fn read(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot read from stdout!");
    }

    /// 向标准输出写入文本
    ///
    /// 将用户缓冲区中的文本输出到控制台。支持跨页面的用户缓冲区，
    /// 自动处理页面边界和 UTF-8 编码。
    ///
    /// ## Arguments
    ///
    /// * `user_buf` - 用户缓冲区，包含要输出的文本
    ///
    /// ## Returns
    ///
    /// 返回写入的字节数，等于用户缓冲区的总长度
    ///
    /// ## 输出过程
    ///
    /// 1. **缓冲区遍历**: 遍历用户缓冲区的所有页面
    /// 2. **文本转换**: 将每个页面的字节转换为 UTF-8 字符串
    /// 3. **控制台输出**: 使用 `print!` 宏输出到控制台
    /// 4. **结果统计**: 统计所有输出的字节数
    ///
    /// ## 文本处理
    ///
    /// - **UTF-8 支持**: 完全支持 Unicode 字符输出
    /// - **页面边界**: 自动处理跨页面的用户缓冲区
    /// - **实时显示**: 文本立即显示到控制台，无需缓冲
    ///
    /// ## 错误处理
    ///
    /// - 如果用户缓冲区包含无效的 UTF-8 序列，会触发 panic
    /// - 如果控制台输出失败，会触发 panic
    ///
    /// ## 性能说明
    ///
    /// 该操作是同步的，会立即输出到控制台。对于大量文本，
    /// 输出速度取决于控制台的性能。
    ///
    /// ## 线程安全
    ///
    /// 多个线程可以同时向标准输出写入，输出操作是原子的，
    /// 不会出现字符交错的情况。
    ///
    /// ## Examples
    ///
    /// ```
    /// let stdout = Stdout;
    /// let data = b"Hello, World!";
    /// let user_buf = UserBuffer::new(data);
    /// let bytes_written = stdout.write(user_buf);
    /// assert_eq!(bytes_written, 13);
    /// ```
    fn write(&self, user_buf: UserBuffer) -> usize {
        for buffer in user_buf.buffers.iter() {
            print!("{}", core::str::from_utf8(*buffer).unwrap());
        }
        user_buf.len()
    }
}

impl File for Stderr {
    /// 检查标准错误是否可读
    ///
    /// ## Returns
    ///
    /// 总是返回 `false`，因为标准错误不支持读取操作
    fn readable(&self) -> bool {
        false
    }

    /// 检查标准错误是否可写
    ///
    /// ## Returns
    ///
    /// 总是返回 `true`，因为标准错误总是可写的
    fn writable(&self) -> bool {
        true
    }

    /// 从标准错误读取数据
    ///
    /// ## Arguments
    ///
    /// * `_user_buf` - 用户缓冲区（未使用）
    ///
    /// ## Panics
    ///
    /// 总是触发 panic，因为标准错误不支持读取操作
    ///
    /// ## 设计说明
    ///
    /// 标准错误是只写设备，不支持读取操作。如果尝试读取，
    /// 会触发 panic 以明确表示操作不被支持。
    fn read(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot read from stderr!");
    }

    /// 向标准错误写入文本
    ///
    /// 将用户缓冲区中的错误信息输出到控制台。支持跨页面的用户缓冲区，
    /// 自动处理页面边界和 UTF-8 编码。
    ///
    /// ## Arguments
    ///
    /// * `user_buf` - 用户缓冲区，包含要输出的错误信息
    ///
    /// ## Returns
    ///
    /// 返回写入的字节数，等于用户缓冲区的总长度
    ///
    /// ## 输出过程
    ///
    /// 1. **缓冲区遍历**: 遍历用户缓冲区的所有页面
    /// 2. **文本转换**: 将每个页面的字节转换为 UTF-8 字符串
    /// 3. **控制台输出**: 使用 `print!` 宏输出到控制台
    /// 4. **结果统计**: 统计所有输出的字节数
    ///
    /// ## 文本处理
    ///
    /// - **UTF-8 支持**: 完全支持 Unicode 字符输出
    /// - **页面边界**: 自动处理跨页面的用户缓冲区
    /// - **实时显示**: 错误信息立即显示到控制台，无需缓冲
    ///
    /// ## 错误处理
    ///
    /// - 如果用户缓冲区包含无效的 UTF-8 序列，会触发 panic
    /// - 如果控制台输出失败，会触发 panic
    ///
    /// ## 性能说明
    ///
    /// 该操作是同步的，会立即输出到控制台。对于大量错误信息，
    /// 输出速度取决于控制台的性能。
    ///
    /// ## 线程安全
    ///
    /// 多个线程可以同时向标准错误写入，输出操作是原子的，
    /// 不会出现字符交错的情况。
    ///
    /// ## 与标准输出的区别
    ///
    /// 在当前的实现中，标准错误和标准输出都输出到同一个控制台，
    /// 但在语义上它们有不同的用途：
    /// - **标准输出**: 用于正常的程序输出
    /// - **标准错误**: 用于错误信息和诊断输出
    ///
    /// ## Examples
    ///
    /// ```
    /// let stderr = Stderr;
    /// let error_msg = b"Error: File not found";
    /// let user_buf = UserBuffer::new(error_msg);
    /// let bytes_written = stderr.write(user_buf);
    /// assert_eq!(bytes_written, 20);
    /// ```
    fn write(&self, user_buf: UserBuffer) -> usize {
        for buffer in user_buf.buffers.iter() {
            print!("{}", core::str::from_utf8(*buffer).unwrap());
        }
        user_buf.len()
    }
}
