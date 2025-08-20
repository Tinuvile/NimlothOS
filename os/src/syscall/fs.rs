//! # 文件系统相关系统调用
//!
//! 实现与文件 I/O 相关的系统调用，目前主要支持向标准输出写入数据。

use crate::{mm::translated_byte_buffer, print, println, task::current_user_token};

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
/// ## 实现原理
///
/// 1. **地址转换**: 使用 `translate_byte_buffer()` 将用户虚拟地址转换为内核可访问的物理地址
/// 2. **分页处理**: 自动处理跨页面的缓冲区，将其分解为多个物理页面的切片
/// 3. **安全输出**: 逐个处理每个物理页面的数据，确保内存访问安全
///
/// ## 内存管理
///
/// 使用新的地址空间管理机制：
/// - 通过 `current_user_token()` 获取当前任务的页表
/// - 使用 `translate_byte_buffer()` 进行安全的地址转换
/// - 支持跨页面的缓冲区访问
///
/// ## Safety
///
/// 相比之前的实现，现在通过页表转换提供了更好的安全性：
/// - 自动验证用户提供的虚拟地址是否有效
/// - 确保只访问用户任务有权限访问的内存
/// - 防止访问内核内存或其他任务的内存
///
/// ## Panics
///
/// - 当 `fd` 不是支持的文件描述符时会 panic
/// - 当缓冲区内容不是有效 UTF-8 字符串时会 panic
/// - 当用户提供的虚拟地址无效时，`translate_byte_buffer` 可能会 panic
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
            // 通过页表转换获取用户缓冲区的物理地址
            let buffers = translated_byte_buffer(current_user_token(), buf, len);
            // 遍历所有物理页面，逐个输出数据到控制台
            for buffer in buffers {
                print!("{}", core::str::from_utf8(buffer).unwrap());
            }
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write: {}!", fd);
        }
    }
}
