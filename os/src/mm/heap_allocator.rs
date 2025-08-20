//! # 内核堆内存分配器
//!
//! 提供动态内存分配功能，支持 `Box`、`Vec` 等需要堆内存的 Rust 标准库功能。
//! 使用 Buddy System 算法进行高效的内存管理。
//!
//! ## 实现细节
//!
//! - **分配算法**: Buddy System，支持快速分配和合并
//! - **内存大小**: 3MB (0x300000 字节)
//! - **最大块大小**: 2^32 = 4GB
//! - **线程安全**: 使用 `LockedHeap` 保证多线程安全

use crate::config::KERNEL_HEAP_SIZE;
use buddy_system_allocator::LockedHeap;
use core::ptr::addr_of_mut;

/// 全局堆分配器实例
///
/// 使用 Buddy System 分配算法，最大支持 2^32 字节的单次分配。
/// 该分配器是线程安全的，内部使用自旋锁保护。
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::empty();

/// 内核堆内存空间
///
/// 静态分配的内存区域，作为动态内存分配的后备存储。
/// 大小为 [`KERNEL_HEAP_SIZE`] (3MB)。
static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 初始化内核堆分配器
///
/// 将静态分配的 [`HEAP_SPACE`] 内存区域注册到堆分配器中，
/// 使其可用于动态内存分配。
///
/// 必须在使用任何需要堆内存的功能（如 `Box`、`Vec`）之前调用。
///
/// ## Safety
///
/// 该函数使用 `unsafe` 代码访问可变静态变量 `HEAP_SPACE`。
/// 调用者必须确保：
/// - 只在内核初始化阶段调用一次
/// - 调用时没有其他代码正在访问 `HEAP_SPACE`
pub fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(
            addr_of_mut!(HEAP_SPACE) as *mut u8 as usize,
            KERNEL_HEAP_SIZE,
        );
    }
}

/// 堆分配错误处理器
///
/// 当动态内存分配失败时，此函数会被调用。
/// 由于操作系统内核中的内存分配失败通常是致命错误，
/// 因此直接触发 panic 来终止系统运行。
///
/// ## Arguments
///
/// * `layout` - 导致分配失败的内存布局信息，包含大小和对齐要求
///
/// ## Panics
///
/// 该函数总是触发 panic，不会返回
#[alloc_error_handler]
pub fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}
