//! # 文件系统相关系统调用
//!
//! 实现与文件 I/O 相关的系统调用，目前主要支持向标准输出写入数据。

use crate::mm::translated_byte_buffer;
use crate::print;
use crate::sbi::console_getchar;
use crate::task::{current_user_token, suspend_current_and_run_next};

/// 标准输出文件描述符
///
/// 对应 UNIX 标准的 `stdout` 文件描述符，用于向控制台输出文本。
const FD_STDIN: usize = 0;
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
    match fd {
        FD_STDOUT => {
            // 通过页表转换获取用户缓冲区的物理地址
            let buffers = translated_byte_buffer(current_user_token(), buf, len);
            // 遍历所有物理页面，逐个输出数据到控制台
            for buffer in buffers {
                if let Ok(string) = core::str::from_utf8(buffer) {
                    print!("{}", string);
                } else {
                    continue;
                }
            }
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write: {}!", fd);
        }
    }
}

/// 系统调用：从文件描述符读取数据
///
/// 实现 `read(2)` 系统调用，从指定的文件描述符中读取数据。
/// 当前实现只支持从标准输入 (stdin) 读取单个字符。
///
/// ## Arguments
///
/// * `fd` - 文件描述符，当前只支持 `FD_STDIN` (0)
/// * `buf` - 指向接收数据的缓冲区指针
/// * `len` - 要读取的字节数，当前限制为 1
///
/// ## Returns
///
/// - 成功时返回实际读取的字节数 (当前总是返回 1)
/// - 失败时会 panic (暂未实现错误码返回)
///
/// ## 实现限制
///
/// 当前实现有以下限制：
/// - **单字符读取**: 只支持 `len = 1`，即每次只能读取一个字符
/// - **仅支持 stdin**: 只能从标准输入文件描述符 (0) 读取
/// - **阻塞读取**: 如果没有输入字符，会挂起当前任务等待输入
///
/// ## 阻塞机制
///
/// ### 字符读取逻辑
/// - 调用 `console_getchar()` 尝试读取字符
/// - 如果返回 0 (无字符可读)，挂起当前任务并切换到其他任务
/// - 当有字符输入时，任务被重新调度，继续读取
/// - 读取到有效字符后跳出循环
///
/// ### 任务调度协作
/// ```rust
/// loop {
///     c = console_getchar();
///     if c == 0 {
///         suspend_current_and_run_next();  // 让出 CPU
///         continue;                        // 被调度回来时继续尝试
///     } else {
///         break;                          // 成功读取字符
///     }
/// }
/// ```
///
/// ## 内存管理
///
/// ### 地址转换安全性
/// - 使用 `current_user_token()` 获取当前任务的页表
/// - 通过 `translated_byte_buffer()` 安全地访问用户内存
/// - 支持跨页面的缓冲区（虽然当前只读取1字节）
///
/// ### 内存写入
/// - 使用 `write_volatile()` 确保写入操作不被编译器优化
/// - 直接写入用户提供的缓冲区的第一个字节
///
/// ## 使用场景
///
/// ### 键盘输入处理
/// ```rust
/// // 用户态代码
/// char ch;
/// read(0, &ch, 1);  // 从键盘读取一个字符
/// printf("You typed: %c\n", ch);
/// ```
///
/// ### 交互式程序
/// ```rust
/// // 简单的交互循环
/// loop {
///     print!("Enter a character: ");
///     let mut buffer = [0u8; 1];
///     read(0, buffer.as_mut_ptr(), 1);
///     if buffer[0] == b'q' {
///         break;  // 输入 'q' 退出
///     }
///     println!("You entered: {}", buffer[0] as char);
/// }
/// ```
///
/// ## Safety
///
/// ### 内存安全
/// - 通过页表转换验证用户地址的有效性
/// - 使用 `write_volatile` 防止编译器优化导致的问题
/// - 确保只写入用户任务有权访问的内存
///
/// ### 并发安全
/// - 阻塞读取通过任务调度实现，不会阻塞整个系统
/// - 任务挂起和恢复由调度器安全管理
///
/// ## 限制和待优化
///
/// 1. **单字符限制**: 每次只能读取一个字符，效率较低
/// 2. **错误处理**: 当前使用 panic，应该返回适当的错误码
/// 3. **缓冲支持**: 未来可以支持读取多个字符到缓冲区
/// 4. **文件系统**: 目前只支持控制台输入，不支持文件读取
///
/// ## Panics
///
/// - 当 `len != 1` 时会 panic
/// - 当 `fd` 不是 `FD_STDIN` 时会 panic
/// - 当用户提供的虚拟地址无效时，地址转换可能会 panic
///
/// ## Examples
///
/// ### C 风格调用
/// ```c
/// char buffer;
/// ssize_t result = read(0, &buffer, 1);  // 从 stdin 读取一个字符
/// if (result == 1) {
///     printf("Read character: %c\n", buffer);
/// }
/// ```
///
/// ### 在系统调用处理中
/// ```rust
/// pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
///     match syscall_id {
///         SYSCALL_READ => sys_read(args[0], args[1] as *const u8, args[2]),
///         // ...
///     }
/// }
/// ```
pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        FD_STDIN => {
            assert_eq!(len, 1, "Only support len = 1 in sys_read!");
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
            let mut buffers = translated_byte_buffer(current_user_token(), buf, len);
            unsafe {
                buffers[0].as_mut_ptr().write_volatile(ch);
            }
            1
        }
        _ => {
            panic!("Unsupported fd in sys_read!");
        }
    }
}
