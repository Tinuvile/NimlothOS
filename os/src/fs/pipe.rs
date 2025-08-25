//! # 管道（Pipe）实现模块
//!
//! 提供用户态进程间通过字节流进行通信的管道实现。管道由一段环形缓冲区
//! 和两个端点组成：只读端与只写端。读端从缓冲区取字节，写端向缓冲区写入
//! 字节，配合调度器实现阻塞式读写与生产者-消费者语义。
//!
//! ## 设计要点
//! - **环形缓冲区**：固定容量（`RING_BUFFER_SIZE`）的字节数组，利用 `head/tail` 指针
//!   与状态位实现无额外拷贝的顺序读写。
//! - **阻塞语义**：
//!   - 当读端在缓冲区为空时阻塞（让出 CPU），直至有数据可读或写端全部关闭。
//!   - 当写端在缓冲区满时阻塞（让出 CPU），直至有空间可写。
//! - **端点生命周期**：通过 `Arc`/`Weak` 追踪写端是否仍存活，读端可在写端全部关闭且
//!   缓冲区为空时返回 EOF。
//! - **并发安全**：内部通过 `UPSafeCell` 提供独占访问；临界区应尽量缩短，阻塞前先释放锁。
//!
//! ## 与文件接口的关系
//! 本模块中的 `Pipe` 实现了内核抽象 `File`，可与标准文件描述符框架无缝协作：
//! - `read(&self, UserBuffer) -> usize`
//! - `write(&self, UserBuffer) -> usize`
//! - `readable()` / `writable()`
//!
//! ## 使用示例
//! 通过系统调用层包装：
//! 1. 进程 A 调用 `pipe()` 获得一对 `fd[0]`（读端）、`fd[1]`（写端）。
//! 2. 父进程 `fork()` 后将写端 `dup`/重定向给子进程标准输出，读端给另一个子进程标准输入。
//! 3. 两个子进程之间即可通过管道字节流进行通信。

use crate::{fs::File, sync::UPSafeCell, task::suspend_current_and_run_next};
use alloc::sync::{Arc, Weak};

const RING_BUFFER_SIZE: usize = 32;

/// 管道端点
///
/// - `readable = true` 表示该端点为读端；`writable = true` 表示该端点为写端。
/// - 端点通过共享的 `PipeRingBuffer` 进行读写。
pub struct Pipe {
    readable: bool,
    writable: bool,
    buffer: Arc<UPSafeCell<PipeRingBuffer>>,
}

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    Full,
    Empty,
    Normal,
}

/// 管道环形缓冲区
///
/// 使用固定大小的数组作为底层存储，`head` 指向下一个可读位置，`tail` 指向下一个可写位置。
/// 通过 `status` 区分满/空/正常，避免歧义。`write_end` 记录写端是否仍存活，用于 EOF 判断。
pub struct PipeRingBuffer {
    arr: [u8; RING_BUFFER_SIZE],
    head: usize,
    tail: usize,
    status: RingBufferStatus,
    write_end: Option<Weak<Pipe>>,
}

impl PipeRingBuffer {
    /// 创建空的环形缓冲区
    pub fn new() -> Self {
        Self {
            arr: [0; RING_BUFFER_SIZE],
            head: 0,
            tail: 0,
            status: RingBufferStatus::Empty,
            write_end: None,
        }
    }

    /// 在缓冲区中记录写端弱引用（用于 EOF 判定）
    pub fn set_write_end(&mut self, write_end: &Arc<Pipe>) {
        self.write_end = Some(Arc::downgrade(write_end));
    }

    /// 从缓冲区读取一个字节（不做空检查，调用方需确保可读）
    pub fn read_byte(&mut self) -> u8 {
        self.status = RingBufferStatus::Normal;
        let c = self.arr[self.head];
        self.head = (self.head + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::Empty;
        }
        c
    }

    /// 向缓冲区写入一个字节（不做满检查，调用方需确保可写）
    pub fn write_byte(&mut self, byte: u8) {
        self.status = RingBufferStatus::Normal;
        self.arr[self.tail] = byte;
        self.tail = (self.tail + 1) % RING_BUFFER_SIZE;
        if self.tail == self.head {
            self.status = RingBufferStatus::Full;
        }
    }

    /// 当前可读字节数
    pub fn available_read(&self) -> usize {
        if self.status == RingBufferStatus::Empty {
            0
        } else if self.tail > self.head {
            self.tail - self.head
        } else {
            self.tail + RING_BUFFER_SIZE - self.head
        }
    }

    /// 当前可写空闲空间大小
    pub fn available_write(&self) -> usize {
        if self.status == RingBufferStatus::Full {
            0
        } else {
            RING_BUFFER_SIZE - self.available_read()
        }
    }

    /// 是否所有写端均已关闭（用于读端在空时返回 EOF）
    pub fn all_write_ends_closed(&self) -> bool {
        self.write_end.as_ref().unwrap().upgrade().is_none()
    }
}

impl Pipe {
    /// 基于共享缓冲区创建读端
    pub fn read_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: true,
            writable: false,
            buffer,
        }
    }

    /// 基于共享缓冲区创建写端
    pub fn write_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: false,
            writable: true,
            buffer,
        }
    }
}

/// 创建一对管道端点（读端、写端）
///
/// 返回 `(read_end, write_end)`，二者共享同一环形缓冲区。
pub fn make_pipe() -> (Arc<Pipe>, Arc<Pipe>) {
    let buffer = Arc::new(unsafe { UPSafeCell::new(PipeRingBuffer::new()) });
    let read_end = Arc::new(Pipe::read_end_with_buffer(buffer.clone()));
    let write_end = Arc::new(Pipe::write_end_with_buffer(buffer.clone()));
    buffer.exclusive_access().set_write_end(&write_end);
    (read_end, write_end)
}

impl File for Pipe {
    /// 是否为可读端
    fn readable(&self) -> bool {
        self.readable
    }

    /// 是否为可写端
    fn writable(&self) -> bool {
        self.writable
    }

    /// 从管道读取到用户缓冲区
    ///
    /// - 若缓冲区为空且写端仍存活：释放锁并让出 CPU，直到有数据或写端关闭。
    /// - 若缓冲区为空且写端全部关闭：返回已读字节数（可能为 0，表示 EOF）。
    fn read(&self, buf: crate::mm::UserBuffer) -> usize {
        assert!(self.readable);
        let want_to_read = buf.len();
        let mut buf_iter = buf.into_iter();
        let mut already_read = 0usize;
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let loop_read = ring_buffer.available_read();
            if loop_read == 0 {
                if ring_buffer.all_write_ends_closed() {
                    return already_read;
                }
                drop(ring_buffer);
                suspend_current_and_run_next();
                continue;
            }
            for _ in 0..loop_read {
                if let Some(byte_ref) = buf_iter.next() {
                    unsafe {
                        *byte_ref = ring_buffer.read_byte();
                    }
                    already_read += 1;
                    if already_read == want_to_read {
                        return want_to_read;
                    }
                } else {
                    return already_read;
                }
            }
        }
    }

    /// 将用户缓冲区写入到管道
    ///
    /// - 若缓冲区已满：释放锁并让出 CPU，直到有空间可写。
    fn write(&self, buf: crate::mm::UserBuffer) -> usize {
        assert!(self.writable);
        let want_to_write = buf.len();
        let mut buf_iter = buf.into_iter();
        let mut already_write = 0usize;
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let loop_write = ring_buffer.available_write();
            if loop_write == 0 {
                drop(ring_buffer);
                suspend_current_and_run_next();
                continue;
            }
            for _ in 0..loop_write {
                if let Some(byte_ref) = buf_iter.next() {
                    ring_buffer.write_byte(unsafe { *byte_ref });
                    already_write += 1;
                    if already_write == want_to_write {
                        return want_to_write;
                    }
                } else {
                    return already_write;
                }
            }
        }
    }
}
