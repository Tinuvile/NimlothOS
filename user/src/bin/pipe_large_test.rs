#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

extern crate alloc;

use alloc::format;
use user_lib::{close, fork, pipe, read, time, wait, write};

const LENGTH: usize = 3000;
#[unsafe(no_mangle)]
pub fn main() -> i32 {
    // 创建管道
    // 父进程写到子进程
    let mut down_pipe_fd = [0usize; 2];
    // 子进程写到父进程
    let mut up_pipe_fd = [0usize; 2];
    pipe(&mut down_pipe_fd);
    pipe(&mut up_pipe_fd);
    let mut random_str = [0u8; LENGTH];
    if fork() == 0 {
        // 关闭写端
        close(down_pipe_fd[1]);
        // 关闭读端
        close(up_pipe_fd[0]);
        assert_eq!(read(down_pipe_fd[0], &mut random_str) as usize, LENGTH);
        close(down_pipe_fd[0]);
        let sum: usize = random_str.iter().map(|v| *v as usize).sum::<usize>();
        println!("sum = {}(child)", sum);
        let sum_str = format!("{}", sum);
        write(up_pipe_fd[1], sum_str.as_bytes());
        close(up_pipe_fd[1]);
        println!("Child process exited!");
        0
    } else {
        // 关闭读端
        close(down_pipe_fd[0]);
        // 关闭写端
        close(up_pipe_fd[1]);
        // 生成一个长随机字符串
        for ch in random_str.iter_mut() {
            *ch = time() as u8;
        }
        // 发送
        assert_eq!(
            write(down_pipe_fd[1], &random_str) as usize,
            random_str.len()
        );
        // 关闭写端
        close(down_pipe_fd[1]);
        // calculate sum(parent)
        let sum: usize = random_str.iter().map(|v| *v as usize).sum::<usize>();
        println!("sum = {}(parent)", sum);
        // 接收子进程的sum
        let mut child_result = [0u8; 32];
        let result_len = read(up_pipe_fd[0], &mut child_result) as usize;
        close(up_pipe_fd[0]);
        // 检查
        assert_eq!(
            sum,
            str::parse::<usize>(core::str::from_utf8(&child_result[..result_len]).unwrap())
                .unwrap()
        );
        let mut _unused: i32 = 0;
        wait(&mut _unused);
        println!("pipe_large_test passed!");
        0
    }
}
