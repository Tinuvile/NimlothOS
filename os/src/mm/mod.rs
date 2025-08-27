//! # 内存管理模块
//!
//! 提供完整的内存管理功能，包括地址管理、页表操作、物理页帧分配、
//! 堆分配器和地址空间管理。支持 RISC-V SV39 分页机制。
//!
//! ## 模块组织
//!
//! - [`address`] - 地址和页号的类型安全封装，支持地址对齐和转换
//! - [`frame_allocator`] - 物理页帧分配器，管理物理内存页面
//! - [`heap_allocator`] - 内核堆分配器，支持动态内存分配
//! - [`memory_set`] - 地址空间管理，支持内存映射和地址空间切换
//! - [`page_table`] - 页表管理，实现虚拟地址到物理地址的转换
//!
//! ## 初始化流程
//!
//! 内存管理系统的初始化顺序：
//! 1. **堆分配器初始化** - 启用内核动态内存分配
//! 2. **页帧分配器初始化** - 设置物理内存管理
//! 3. **内核地址空间激活** - 启用虚拟内存管理
//!
//! ## 核心类型
//!
//! ### 地址类型
//! - [`PhysAddr`] / [`VirtAddr`] - 物理地址和虚拟地址
//! - [`PhysPageNum`] / [`VirtPageNum`] - 物理页号和虚拟页号
//!
//! ### 页表管理
//! - [`PageTable`] - 多级页表结构
//! - [`PageTableEntry`] - 页表项
//! - [`PTEFlags`] - 页表项标志位
//!
//! ### 内存分配
//! - [`FrameTracker`] - 物理页帧的RAII管理
//! - [`frame_alloc`] / [`frame_dealloc`] - 页帧分配和释放
//!
//! ### 地址空间管理
//! - [`MemorySet`] - 完整的地址空间
//! - [`MapPermission`] - 内存访问权限
//! - [`KERNEL_SPACE`] - 全局内核地址空间
//!
//! ## 使用示例
//!
//! ```rust
//! // 初始化内存管理系统
//! mm::init();
//!
//! // 分配物理页帧
//! let frame = frame_alloc().unwrap();
//! let ppn = frame.ppn;
//!
//! // 地址转换
//! let va = VirtAddr::from(0x10000000);
//! let vpn = va.floor();
//!
//! // 访问内核地址空间
//! let mut kernel_space = KERNEL_SPACE.exclusive_access();
//! kernel_space.activate();
//! ```

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, StepByOne, VPNRange, VirtAddr, VirtPageNum};
pub use frame_allocator::{FrameTracker, frame_alloc, frame_dealloc};
pub use memory_set::{KERNEL_SPACE, MapPermission, MemorySet, kernel_token};
pub use page_table::{
    PageTable, PageTableEntry, UserBuffer, translated_byte_buffer, translated_ref,
    translated_refmut, translated_str,
};

/// 初始化内存管理系统
///
/// 按照正确的顺序初始化内存管理的各个组件，确保系统能够正常进行
/// 内存分配和虚拟地址转换。
///
/// ## 初始化步骤
///
/// 1. **堆分配器初始化** - 调用 [`heap_allocator::init_heap()`]
///    - 设置内核堆的起始地址和大小
///    - 启用 `alloc` crate 的动态内存分配功能
///    - 支持 `Vec`、`BTreeMap` 等集合类型
///
/// 2. **页帧分配器初始化** - 调用 [`frame_allocator::init_frame_allocator()`]
///    - 计算可分配的物理内存范围
///    - 初始化物理页帧的空闲列表
///    - 设置页帧分配和回收机制
///
/// 3. **内核地址空间激活** - 调用 [`KERNEL_SPACE.exclusive_access().activate()`]
///    - 创建内核地址空间的页表映射
///    - 设置 `satp` 寄存器启用分页机制
///    - 刷新 TLB 确保地址转换正确
///
/// ## 调用时机
///
/// 此函数应在内核初始化的早期阶段调用，通常在 BSS 段清零和日志系统
/// 初始化之后，但在任何需要动态内存分配的操作之前。
///
/// ## Panics
///
/// 如果内存管理系统初始化失败（如物理内存不足、页表创建失败等），
/// 函数会触发 panic，因为这是系统运行的基础要求。
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
