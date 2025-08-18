//! # 文件系统相关系统调用
//!
//! 实现与文件 I/O 相关的系统调用，目前主要支持向标准输出写入数据。

use crate::{print, println};

/// 标准输出文件描述符
///
/// 对应 UNIX 标准的 `stdout` 文件描述符，用于向控制台输出文本。
const FD_STDOUT: usize = 1;

/// 系统调用：向文件描述符写入数据
///
/// 实现 `write(2)` 系统调用，将指定的数据写入到文件描述符中。
/// 当前实现只支持向标准输出 (stdout) 写入。
///
/// ## Arguments
///
/// * `fd` - 文件描述符，当前只支持 `FD_STDOUT` (1)
/// * `buf` - 指向要写入数据的缓冲区指针
/// * `len` - 要写入的字节数
///
/// ## Returns
///
/// - 成功时返回实际写入的字节数
/// - 失败时返回负值错误码
///
/// ## Safety
///
/// 该函数使用 `unsafe` 代码：
/// - 从原始指针 `buf` 创建切片
/// - 假设用户提供的指针和长度是有效的
///
/// ## Panics
///
/// - 当 `fd` 不是支持的文件描述符时会 panic
/// - 当缓冲区内容不是有效 UTF-8 字符串时会 panic
///
/// ## Security Note
///
/// 当前实现跳过了内存访问权限检查（注释掉的代码），
/// 在生产环境中应该验证用户提供的指针是否在合法的内存范围内。
///
/// ## Examples
///
/// 从用户态调用：
/// ```c
/// write(1, "Hello, World!", 13);  // 向 stdout 输出字符串
/// ```
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    /*
    // TODO: 重新启用内存访问权限检查
    let app_range = get_current_app_range();
    let user_stack_range = get_user_stack_range();

    let buf_start = buf as usize;
    let buf_end = buf_start + len;

    let in_app_range = buf_start >= app_range.0 && buf_end <= app_range.1;
    let in_user_stack_range = buf_start >= user_stack_range.0 && buf_end <= user_stack_range.1;

    if !in_app_range && !in_user_stack_range {
        println!(
            "[kernel] sys_write: buffer out of range [0x{:x}, 0x{:x})",
            buf_start, buf_end
        );
        println!(
            "app_range: [0x{:x}, 0x{:x}), user_stack_range: [0x{:x}, 0x{:x})",
            app_range.0, app_range.1, user_stack_range.0, user_stack_range.1
        );
        sys_exit(-1);
    }
    */

    match fd {
        FD_STDOUT => {
            // 从用户空间读取数据并输出到控制台
            let slice = unsafe { core::slice::from_raw_parts(buf, len) };
            let str = core::str::from_utf8(slice).unwrap();
            print!("{}", str);
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write: {}!", fd);
        }
    }
}
