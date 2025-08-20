//! # 内存集合管理模块
//!
//! 提供完整的地址空间管理功能，包括内存映射区域的创建、管理和地址空间切换。
//! 支持 RISC-V SV39 分页机制，实现内核和用户程序的地址空间隔离。
//!
//! ## 核心概念
//!
//! - [`MemorySet`] - 完整的地址空间，包含页表和多个内存映射区域
//! - [`MapArea`] - 单个连续的内存映射区域，具有统一的映射类型和权限
//! - [`MapType`] - 映射类型：恒等映射或帧映射
//! - [`MapPermission`] - 内存访问权限：读、写、执行、用户态访问
//!
//! ## 地址空间布局
//!
//! ### 内核地址空间 (低256G)
//! ```text
//! ┌──────────────────────────────────--───────────────────────────┐
//! │                   Kernel Address Space                        │
//! ├─────────────────┬──────────────-─┬──────────-───┬─────────────┤
//! │  .text Section  │ .rodata Section│ .data Section│ .bss Section│
//! │     (R+X)       │      (R)       │    (R+W)     │    (R+W)    │
//! ├─────────────────┴───────────────-┴───────────-──┴─────────────┤
//! │                  Physical Memory Mapping                      │
//! │                        (R+W)                                  │
//! └─────────────────────────────────────────────────--────────────┘
//! ```
//!
//! ### 用户地址空间
//! ```text
//! 高地址 (TRAMPOLINE)
//! ┌──────────────────────────────────────────────────┐
//! │                   Trampoline                     │
//! │                    (R+X)                         │
//! ├──────────────────────────────────────────────────┤
//! │                 Trap Context                     │
//! │                    (R+W)                         │
//! ├──────────────────────────────────────────────────┤
//! │                  User Stack                      │
//! │                   (R+W+U)                        │
//! ├──────────────────────── ─────────────────────────┤
//! │                     ...                          │
//! ├──────────────────────────────────────────────────┤
//! │               Program Sections                   │
//! │            (.text/.data/.bss etc)                │
//! │              (Based on ELF flags)                │
//! └──────────────────────────────────────────────────┘
//! 低地址 (0x10000)
//! ```
//!
//! ## 映射类型
//!
//! ### Identical 映射 (恒等映射)
//! - **特点**: 虚拟地址 = 物理地址
//! - **用途**: 内核直接访问物理内存和设备
//! - **优势**: 简单、高效、地址可预测
//! - **限制**: 无法提供地址空间隔离
//!
//! ### Framed 映射 (帧映射)
//! - **特点**: 每个虚拟页映射到独立分配的物理页帧
//! - **用途**: 用户程序地址空间、内核堆
//! - **优势**: 灵活、安全、支持地址空间隔离
//! - **成本**: 需要额外的页表和页帧管理
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::mm::{MemorySet, MapArea, MapType, MapPermission};
//!
//! // 创建新的地址空间
//! let mut memory_set = MemorySet::new_bare();
//!
//! // 添加内存映射区域
//! memory_set.insert_framed_area(
//!     VirtAddr::from(0x10000000),
//!     VirtAddr::from(0x10001000),
//!     MapPermission::R | MapPermission::W | MapPermission::U
//! );
//!
//! // 激活地址空间
//! memory_set.activate();
//! ```

use super::{
    FrameTracker, PhysAddr, PhysPageNum, StepByOne, VPNRange, VirtAddr, VirtPageNum, frame_alloc,
    frame_dealloc,
    page_table::{PTEFlags, PageTable, PageTableEntry},
};
use crate::config::{MEMORY_END, MMIO, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT, USER_STACK_SIZE};
use crate::println;
use crate::sync::UPSafeCell;
use alloc::collections::btree_map::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::bitflags;
use core::arch::asm;
use lazy_static::lazy_static;
use riscv::register::satp;

unsafe extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// 全局内核地址空间
    ///
    /// 系统中唯一的内核地址空间实例，包含内核代码段、数据段、物理内存映射等。
    /// 使用 `Arc<UPSafeCell<_>>` 确保在单处理器环境下的安全并发访问。
    ///
    /// ## 初始化时机
    ///
    /// 在 `mm::init()` 函数中通过 `KERNEL_SPACE.exclusive_access().activate()` 激活。
    ///
    /// ## 使用方式
    ///
    /// ```rust
    /// // 获取内核地址空间的独占访问权限
    /// let mut kernel_space = KERNEL_SPACE.exclusive_access();
    /// kernel_space.activate(); // 切换到内核地址空间
    /// ```
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> =
        Arc::new(unsafe { UPSafeCell::new(MemorySet::new_kernel()) });
}

/// 内存映射区域
///
/// 表示地址空间中一个连续的虚拟内存区域，具有统一的映射类型和访问权限。
/// 每个 `MapArea` 管理一段虚拟页号范围，并维护其到物理页帧的映射关系。
///
/// ## 字段说明
///
/// - `vpn_range`: 虚拟页号范围，定义区域的边界
/// - `data_frames`: 虚拟页号到物理页帧的映射表（仅用于 Framed 映射）
/// - `map_type`: 映射类型（恒等映射或帧映射）
/// - `map_perm`: 访问权限（读/写/执行/用户态）
///
/// ## 设计原理
///
/// `MapArea` 将连续的虚拟地址范围抽象为统一的管理单元，简化了
/// 大块内存区域的映射和权限管理。通过 `BTreeMap` 维护页面映射关系，
/// 既保证了查找效率，又支持有序遍历。
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

/// 内存集合（地址空间）
///
/// 表示一个完整的虚拟地址空间，包含页表和多个内存映射区域。
/// 每个进程都有独立的 `MemorySet`，实现地址空间隔离。
///
/// ## 组成部分
///
/// - `page_table`: 多级页表，负责虚拟地址到物理地址的转换
/// - `areas`: 内存映射区域列表，每个区域有独立的映射类型和权限
///
/// ## 生命周期管理
///
/// `MemorySet` 拥有其包含的所有页表和物理页帧，当对象被销毁时，
/// 会自动释放相关的物理内存资源，确保内存安全。
///
/// ## 并发安全
///
/// 内核地址空间通过 `KERNEL_SPACE` 全局变量管理，使用 `UPSafeCell`
/// 提供单处理器环境下的安全可变访问。
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}

/// 内存映射类型
///
/// 定义虚拟页面到物理页帧的映射方式，影响地址转换的行为和性能。
///
/// ## 映射类型对比
///
/// | 类型 | 地址关系 | 用途 | 优势 | 劣势 |
/// |------|----------|------|------|------|
/// | Identical | VA = PA | 内核段、设备 | 简单、高效 | 无隔离 |
/// | Framed | VA ≠ PA | 用户程序、堆 | 灵活、安全 | 复杂、开销 |
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MapType {
    /// 恒等映射
    ///
    /// 虚拟地址直接等于物理地址，不需要额外的物理页帧分配。
    /// 主要用于内核代码段、数据段以及设备内存映射。
    ///
    /// ## 特点
    /// - 地址转换开销最小
    /// - 地址可预测，便于调试
    /// - 适合需要直接访问物理地址的场景
    Identical,

    /// 帧映射
    ///
    /// 每个虚拟页面映射到独立分配的物理页帧，提供完整的
    /// 地址空间隔离和内存保护功能。
    ///
    /// ## 特点
    /// - 支持地址空间隔离
    /// - 可以实现写时复制、懒加载等高级功能
    /// - 需要维护虚拟页到物理页的映射表
    Framed,
}

bitflags! {
    /// 内存映射权限
    ///
    /// 定义内存区域的访问权限，直接对应 RISC-V 页表项的权限位。
    /// 权限检查由硬件 MMU 执行，违反权限将触发页面异常。
    ///
    /// ## 权限位布局
    ///
    /// ```text
    /// 权限位布局:
    /// ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┐
    /// │  7  │  6  │  5  │  4  │  3  │  2  │  1  │  0  │
    /// └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┘
    ///                     ↑     ↑     ↑     ↑
    ///                     U     X     W     R
    /// ```
    ///
    /// ## 常用权限组合
    ///
    /// - `R`: 只读数据段 (.rodata)
    /// - `R | W`: 可读写数据段 (.data, .bss)
    /// - `R | X`: 只读代码段 (.text)
    /// - `R | W | U`: 用户态可读写区域
    /// - `R | W | X | U`: 用户态全权限区域
    ///
    /// ## 安全约束
    ///
    /// - 不能单独设置 `W` 权限（RISC-V 规范要求）
    /// - `U` 权限允许用户态访问，否则仅内核态可访问
    /// - 权限检查在每次内存访问时由硬件执行
    pub struct MapPermission: u8 {
        /// 可读权限 (Read)
        ///
        /// 允许从该内存区域读取数据。几乎所有有用的内存区域
        /// 都需要此权限，除非是特殊的只写设备寄存器。
        const R = 1 << 1;

        /// 可写权限 (Write)
        ///
        /// 允许向该内存区域写入数据。根据 RISC-V 规范，
        /// 设置写权限时必须同时设置读权限。
        const W = 1 << 2;

        /// 可执行权限 (Execute)
        ///
        /// 允许从该内存区域获取并执行指令。通常用于
        /// 代码段，不应与写权限同时设置（W^X 原则）。
        const X = 1 << 3;

        /// 用户态访问权限 (User)
        ///
        /// 允许用户态程序访问该内存区域。不设置此权限时，
        /// 只有内核态（S-mode）可以访问。
        const U = 1 << 4;
    }
}

impl MapArea {
    /// 创建新的内存映射区域
    ///
    /// 根据指定的虚拟地址范围、映射类型和访问权限创建一个新的内存映射区域。
    /// 地址范围会自动对齐到页边界。
    ///
    /// ## Arguments
    ///
    /// * `start_va` - 区域起始虚拟地址
    /// * `end_va` - 区域结束虚拟地址（不包含）
    /// * `map_type` - 映射类型（恒等映射或帧映射）
    /// * `map_perm` - 内存访问权限
    ///
    /// ## Returns
    ///
    /// 新创建的内存映射区域，尚未进行实际的页面映射
    ///
    /// ## 地址对齐
    ///
    /// - `start_va` 向下对齐到页边界（使用 `floor()`）
    /// - `end_va` 向上对齐到页边界（使用 `ceil()`）
    /// - 确保整个区域以页为单位进行管理
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 创建用户态可读写区域
    /// let area = MapArea::new(
    ///     VirtAddr::from(0x10000000),
    ///     VirtAddr::from(0x10001000),
    ///     MapType::Framed,
    ///     MapPermission::R | MapPermission::W | MapPermission::U
    /// );
    ///
    /// // 创建内核代码段（恒等映射）
    /// let text_area = MapArea::new(
    ///     VirtAddr::from(stext as usize),
    ///     VirtAddr::from(etext as usize),
    ///     MapType::Identical,
    ///     MapPermission::R | MapPermission::X
    /// );
    /// ```
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }

    /// 将整个内存区域映射到页表
    ///
    /// 遍历区域内的所有虚拟页号，为每个页面建立虚拟地址到物理地址的映射。
    /// 根据映射类型的不同，采用不同的物理页面分配策略。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 目标页表的可变引用
    ///
    /// ## 映射行为
    ///
    /// - **Identical映射**: 虚拟页号直接作为物理页号使用
    /// - **Framed映射**: 为每个虚拟页面分配新的物理页帧
    ///
    /// ## Panics
    ///
    /// 如果物理页帧分配失败（内存不足），函数会触发 panic
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let mut area = MapArea::new(
    ///     VirtAddr::from(0x10000000),
    ///     VirtAddr::from(0x10001000),
    ///     MapType::Framed,
    ///     MapPermission::R | MapPermission::W
    /// );
    /// let mut page_table = PageTable::new();
    /// area.map(&mut page_table); // 建立所有页面映射
    /// ```
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }

    /// 从页表中取消整个内存区域的映射
    ///
    /// 遍历区域内的所有虚拟页号，移除每个页面的虚拟地址到物理地址映射。
    /// 对于 Framed 映射，还会释放对应的物理页帧。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 目标页表的可变引用
    ///
    /// ## 清理行为
    ///
    /// - **Identical映射**: 仅移除页表项，不释放物理页面
    /// - **Framed映射**: 移除页表项并释放所有分配的物理页帧
    ///
    /// ## 内存安全
    ///
    /// 函数确保所有相关的物理内存资源得到正确释放，防止内存泄漏。
    /// 通过 `FrameTracker` 的 RAII 机制自动管理页帧生命周期。
    #[allow(unused)]
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }

    /// 将数据复制到映射区域
    ///
    /// 将给定的字节数据复制到已映射的虚拟内存区域中。数据会按页面大小分块复制，
    /// 跨越多个物理页帧。仅支持 Framed 映射类型。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 用于地址转换的页表引用
    /// * `data` - 要复制的源数据
    ///
    /// ## 复制过程
    ///
    /// 1. 从区域起始虚拟页号开始
    /// 2. 每次复制一个页面大小的数据（最多 4KB）
    /// 3. 通过页表转换获取物理页帧地址
    /// 4. 直接写入物理内存
    ///
    /// ## Panics
    ///
    /// - 如果映射类型不是 `MapType::Framed`
    /// - 如果虚拟地址转换失败
    /// - 如果数据长度超出区域范围
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 创建并映射区域
    /// let mut area = MapArea::new(
    ///     VirtAddr::from(0x10000000),
    ///     VirtAddr::from(0x10002000),
    ///     MapType::Framed,
    ///     MapPermission::R | MapPermission::W
    /// );
    /// area.map(&mut page_table);
    ///
    /// // 复制 ELF 文件数据
    /// let elf_data = &[0x7f, 0x45, 0x4c, 0x46, /* ... */];
    /// area.copy_data(&page_table, elf_data);
    /// ```
    pub fn copy_data(&mut self, page_table: &PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }

    /// 映射单个虚拟页面
    ///
    /// 为指定的虚拟页号建立到物理页帧的映射。根据映射类型的不同，
    /// 采用不同的物理页面分配策略。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 目标页表的可变引用
    /// * `vpn` - 要映射的虚拟页号
    ///
    /// ## 映射策略
    ///
    /// ### Identical 映射
    /// - 直接将虚拟页号作为物理页号使用
    /// - 不需要分配新的物理页帧
    /// - 不更新 `data_frames` 映射表
    ///
    /// ### Framed 映射
    /// - 分配新的物理页帧
    /// - 将虚拟页号到页帧的映射记录在 `data_frames` 中
    /// - 通过 `FrameTracker` 管理页帧生命周期
    ///
    /// ## Panics
    ///
    /// - Framed 映射时如果物理页帧分配失败
    /// - 权限位转换失败
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 映射单个页面
    /// let vpn = VirtPageNum(0x10000);
    /// area.map_one(&mut page_table, vpn);
    /// ```
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits()).unwrap();
        page_table.map(vpn, ppn, pte_flags);
    }

    /// 取消单个虚拟页面的映射
    ///
    /// 移除指定虚拟页号到物理页帧的映射。对于 Framed 映射，
    /// 还会释放对应的物理页帧。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 目标页表的可变引用
    /// * `vpn` - 要取消映射的虚拟页号
    ///
    /// ## 清理策略
    ///
    /// ### Identical 映射
    /// - 仅从页表中移除页表项
    /// - 不释放任何物理页面（因为不是独占所有）
    ///
    /// ### Framed 映射
    /// - 从 `data_frames` 中移除映射记录
    /// - 调用 `frame_dealloc()` 释放物理页帧
    /// - 从页表中移除页表项
    ///
    /// ## 内存安全
    ///
    /// 通过 `FrameTracker` 的 RAII 机制自动管理物理页帧的生命周期，
    /// 确保在取消映射时正确释放内存资源。
    ///
    /// ## Panics
    ///
    /// 如果 Framed 映射中找不到对应的页帧记录
    #[allow(unused)]
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        match self.map_type {
            MapType::Identical => {}
            MapType::Framed => {
                let frame = self.data_frames.remove(&vpn).unwrap();
                // frame_dealloc(frame.ppn);
            }
        }
        page_table.unmap(vpn);
    }

    /// 缩小内存区域到指定结束位置
    ///
    /// 将内存区域的结束边界缩小到指定的虚拟页号，取消被移除部分的所有映射。
    /// 主要用于内存回收和动态内存管理。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 目标页表的可变引用
    /// * `new_end` - 新的结束虚拟页号（不包含）
    ///
    /// ## 操作过程
    ///
    /// 1. 计算需要移除的页面范围：`[new_end, old_end)`
    /// 2. 对范围内的每个页面调用 `unmap_one()`
    /// 3. 更新区域的 `vpn_range` 边界
    ///
    /// ## 内存安全
    ///
    /// 所有被移除的 Framed 映射页面对应的物理页帧会被自动释放，
    /// 防止内存泄漏。
    ///
    /// ## Panics
    ///
    /// 如果 `new_end` 大于当前区域的结束位置
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 原区域：[0x1000, 0x3000)
    /// // 缩小到：[0x1000, 0x2000)
    /// area.shrink_to(&mut page_table, VirtPageNum(0x2000));
    /// // 页面 [0x2000, 0x3000) 被取消映射并释放
    /// ```
    #[allow(unused)]
    pub fn shrink_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(new_end, self.vpn_range.get_end()) {
            self.unmap_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }

    /// 扩展内存区域到指定结束位置
    ///
    /// 将内存区域的结束边界扩展到指定的虚拟页号，为新增部分建立映射。
    /// 主要用于动态内存分配和堆空间扩展。
    ///
    /// ## Arguments
    ///
    /// * `page_table` - 目标页表的可变引用
    /// * `new_end` - 新的结束虚拟页号（不包含）
    ///
    /// ## 操作过程
    ///
    /// 1. 计算需要添加的页面范围：`[old_end, new_end)`
    /// 2. 对范围内的每个页面调用 `map_one()`
    /// 3. 更新区域的 `vpn_range` 边界
    ///
    /// ## 内存分配
    ///
    /// 对于 Framed 映射，会为每个新页面分配独立的物理页帧。
    /// 对于 Identical 映射，直接使用对应的物理地址。
    ///
    /// ## Panics
    ///
    /// - 如果 `new_end` 小于当前区域的结束位置
    /// - Framed 映射时如果物理页帧分配失败
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 原区域：[0x1000, 0x2000)
    /// // 扩展到：[0x1000, 0x3000)
    /// area.append_to(&mut page_table, VirtPageNum(0x3000));
    /// // 页面 [0x2000, 0x3000) 被映射并分配
    /// ```
    #[allow(unused)]
    pub fn append_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(self.vpn_range.get_end(), new_end) {
            self.map_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
}

impl MemorySet {
    /// 创建空的地址空间
    ///
    /// 创建一个仅包含空页表和空区域列表的地址空间。这是构建
    /// 复杂地址空间的基础，需要后续添加具体的内存区域。
    ///
    /// ## Returns
    ///
    /// 新创建的空地址空间，不包含任何内存映射
    ///
    /// ## 初始状态
    ///
    /// - `page_table`: 新的空页表，仅包含根页表
    /// - `areas`: 空的区域列表
    ///
    /// ## 使用场景
    ///
    /// - 构建用户程序地址空间
    /// - 构建内核地址空间的基础
    /// - 清理和重建地址空间
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let mut memory_set = MemorySet::new_bare();
    /// // 添加具体的内存区域...
    /// memory_set.insert_framed_area(
    ///     VirtAddr::from(0x10000000),
    ///     VirtAddr::from(0x10001000),
    ///     MapPermission::R | MapPermission::W | MapPermission::U
    /// );
    /// ```
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }

    /// 将内存区域添加到地址空间
    ///
    /// 将一个内存映射区域添加到当前地址空间中，并可选地将数据复制到该区域。
    /// 这是地址空间构建的核心操作。
    ///
    /// ## Arguments
    ///
    /// * `map_area` - 要添加的内存映射区域
    /// * `data` - 可选的初始化数据（如 ELF 段数据）
    ///
    /// ## 操作流程
    ///
    /// 1. **建立映射**: 调用 `map_area.map()` 在页表中建立所有页面映射
    /// 2. **复制数据**: 如果提供了数据，将其复制到映射的内存区域
    /// 3. **记录区域**: 将区域添加到 `areas` 列表中以便管理
    ///
    /// ## 内存安全
    ///
    /// - 区域被添加到 `areas` 列表后，其生命周期由 `MemorySet` 管理
    /// - 当 `MemorySet` 被销毁时，所有区域的映射和页帧会自动清理
    ///
    /// ## 使用场景
    ///
    /// - 添加 ELF 文件的代码段和数据段
    /// - 添加用户栈区域
    /// - 添加内核的各个逻辑段
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&self.page_table, data);
        }
        self.areas.push(map_area);
    }

    /// 插入帧映射内存区域
    ///
    /// 在地址空间中添加一个使用 Framed 映射类型的内存区域。
    /// 该区域的每个页面都会分配独立的物理页帧。
    ///
    /// ## Arguments
    ///
    /// * `start_va` - 区域起始虚拟地址
    /// * `end_va` - 区域结束虚拟地址（不包含）
    /// * `perm` - 内存访问权限
    ///
    /// ## 特点
    ///
    /// - **独立分配**: 每个虚拟页面对应一个新分配的物理页帧
    /// - **地址隔离**: 虚拟地址和物理地址没有固定关系
    /// - **内存保护**: 支持完整的内存访问权限控制
    /// - **自动清理**: 区域销毁时自动释放所有物理页帧
    ///
    /// ## 使用场景
    ///
    /// - 用户程序的堆区域
    /// - 用户程序的栈区域
    /// - 内核的动态内存区域
    /// - 需要地址隔离的任何区域
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 添加用户态可读写区域
    /// memory_set.insert_framed_area(
    ///     VirtAddr::from(0x10000000),
    ///     VirtAddr::from(0x10001000),
    ///     MapPermission::R | MapPermission::W | MapPermission::U
    /// );
    ///
    /// // 添加用户栈区域
    /// memory_set.insert_framed_area(
    ///     VirtAddr::from(USER_STACK_BASE),
    ///     VirtAddr::from(USER_STACK_BASE + USER_STACK_SIZE),
    ///     MapPermission::R | MapPermission::W | MapPermission::U
    /// );
    /// ```
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        perm: MapPermission,
    ) {
        self.push(MapArea::new(start_va, end_va, MapType::Framed, perm), None);
    }

    /// 映射 Trampoline 页面
    ///
    /// 在地址空间的高地址区域映射 Trampoline 页面，用于内核态和用户态之间的
    /// 上下文切换。Trampoline 是一个特殊的汇编代码段，在所有地址空间中都映射到
    /// 同一个物理地址。
    ///
    /// ## 映射特点
    ///
    /// - **虚拟地址**: `TRAMPOLINE` 常量定义的高地址
    /// - **物理地址**: `strampoline` 符号指向的物理地址
    /// - **权限**: 可读 + 可执行（`R | X`）
    /// - **共享**: 所有地址空间都映射到同一物理页面
    ///
    /// ## 作用
    ///
    /// Trampoline 允许在地址空间切换过程中继续执行代码，解决了
    /// 在切换 `satp` 寄存器后指令取指地址变化的问题。
    ///
    /// ## 安全性
    ///
    /// Trampoline 代码经过精心设计，不会泄露内核信息给用户程序。
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }

    /// 创建内核地址空间
    ///
    /// 构建完整的内核地址空间，包括内核的各个逻辑段（.text、.rodata、.data、.bss）
    /// 以及物理内存映射区域和 Trampoline。所有内核段都使用恒等映射，确保虚拟地址
    /// 等于物理地址，便于内核直接访问物理内存。
    ///
    /// ## Returns
    ///
    /// 完整配置的内核地址空间，包含所有必要的内存映射
    ///
    /// ## 内核段映射
    ///
    /// | 段名称 | 权限 | 映射类型 | 用途 |
    /// |--------|------|----------|------|
    /// | .text | R+X | Identical | 内核代码段 |
    /// | .rodata | R | Identical | 只读数据段 |
    /// | .data | R+W | Identical | 已初始化数据段 |
    /// | .bss | R+W | Identical | 未初始化数据段 |
    /// | Physical Memory | R+W | Identical | 物理内存直接映射 |
    /// | Trampoline | R+X | Identical | 上下文切换代码 |
    ///
    /// ## 地址范围
    ///
    /// - **内核段**: 由链接器脚本 `linker-qemu.ld` 定义的符号确定边界
    /// - **物理内存**: 从 `ekernel` 到 `MEMORY_END` 的整个可用物理内存
    /// - **Trampoline**: 固定映射到 `TRAMPOLINE` 虚拟地址
    ///
    /// ## 调试输出
    ///
    /// 函数会打印各个段的地址范围，便于调试和验证内存布局。
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 创建内核地址空间（通常在系统初始化时调用）
    /// let kernel_space = MemorySet::new_kernel();
    ///
    /// // 输出示例：
    /// // .text [0x80200000, 0x80210000)
    /// // .rodata [0x80210000, 0x80220000)
    /// // .data [0x80220000, 0x80230000)
    /// // .bss [0x80230000, 0x80240000)
    /// ```
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        memory_set.map_trampoline();
        println!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        println!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        println!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        println!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        println!("mapping .text section");
        memory_set.push(
            MapArea::new(
                (stext as usize).into(),
                (etext as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            None,
        );
        println!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        );
        println!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        println!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        println!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        memory_set
    }

    /// 从 ELF 文件创建用户地址空间
    ///
    /// 解析 ELF 文件并构建相应的用户程序地址空间，包括程序的各个段、用户栈、
    /// Trap Context 和 Trampoline。所有用户段都使用 Framed 映射，实现地址空间隔离。
    ///
    /// ## Arguments
    ///
    /// * `elf_data` - ELF 文件的二进制数据
    ///
    /// ## Returns
    ///
    /// 返回一个三元组：
    /// - `MemorySet`: 构建好的用户地址空间
    /// - `usize`: 用户栈顶地址
    /// - `usize`: 程序入口点地址
    ///
    /// ## 地址空间布局
    ///
    /// ```text
    /// 高地址 (TRAMPOLINE)
    /// ┌──────────────────────────────────────────────────────┐
    /// │                   Trampoline                         │
    /// │                    (R+X)                             │
    /// ├──────────────────────────────────────────────────────┤
    /// │                 Trap Context                         │
    /// │                    (R+W)                             │
    /// ├──────────────────────────────────────────────────────┤
    /// │                  User Stack                          │
    /// │                   (R+W+U)                            │
    /// ├──────────────────────────────────────────────────────┤
    /// │                  Guard Page                          │
    /// ├──────────────────────────────────────────────────────┤
    /// │               ELF Program Sections                   │
    /// │            (.text/.data/.bss etc)                    │
    /// │              (Based on ELF flags)                    │
    /// └──────────────────────────────────────────────────────┘
    /// 低地址 (0x10000)
    /// ```
    ///
    /// ## ELF 解析过程
    ///
    /// 1. **验证 ELF 魔数**: 确保文件格式正确
    /// 2. **解析程序头**: 遍历所有 `LOAD` 类型的程序段
    /// 3. **权限转换**: 将 ELF 段标志转换为 `MapPermission`
    /// 4. **段映射**: 为每个段创建 Framed 映射并复制数据
    /// 5. **用户栈**: 在程序段之上分配用户栈空间
    /// 6. **系统区域**: 映射 Trap Context 和 Trampoline
    ///
    /// ## 权限映射
    ///
    /// ELF 段标志到内存权限的转换：
    /// - `PF_R` → `MapPermission::R`
    /// - `PF_W` → `MapPermission::W`
    /// - `PF_X` → `MapPermission::X`
    /// - 所有用户段都包含 `MapPermission::U`
    ///
    /// ## 内存安全
    ///
    /// - 所有用户段使用 Framed 映射，与内核地址空间完全隔离
    /// - 用户栈与程序段之间有保护页面防止栈溢出
    /// - Trap Context 仅内核可写，用户只读
    ///
    /// ## Panics
    ///
    /// - ELF 魔数验证失败
    /// - ELF 文件格式错误
    /// - 物理页帧分配失败
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let app_data = get_app_data(0); // 获取应用程序 ELF 数据
    /// let (memory_set, user_stack_top, entry_point) = MemorySet::from_elf(app_data);
    ///
    /// println!("Entry point: {:#x}", entry_point);
    /// println!("User stack top: {:#x}", user_stack_top);
    /// ```
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();

        memory_set.map_trampoline();

        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        // elf.header.pt1：固定格式部分，pt2：可变格式部分
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }

        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into();

        user_stack_bottom += PAGE_SIZE;
        let user_stack_top: usize = user_stack_bottom + USER_STACK_SIZE;
        memory_set.push(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );

        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        (
            memory_set,
            user_stack_top,
            elf.header.pt2.entry_point() as usize,
        )
    }

    /// 激活地址空间
    ///
    /// 将当前地址空间设置为活跃的地址空间，启用该地址空间的页表进行
    /// 地址转换。这是地址空间切换的核心操作。
    ///
    /// ## 操作流程
    ///
    /// 1. **获取页表标识**: 调用 `page_table.token()` 获取 `satp` 寄存器值
    /// 2. **设置 satp**: 将页表标识写入 `satp` 寄存器
    /// 3. **刷新 TLB**: 执行 `sfence.vma` 指令清空 TLB 缓存
    ///
    /// ## satp 寄存器格式
    ///
    /// ```text
    /// ┌─────────────┬─────────────────┬─────────────────────────────────────────────┐
    /// │    MODE     │      ASID       │                    PPN                      │
    /// │   (4bit)    │     (16bit)     │                  (44bit)                    │
    /// └─────────────┴─────────────────┴─────────────────────────────────────────────┘
    /// ```
    ///
    /// ## 安全性
    ///
    /// 此操作使用 `unsafe` 代码，因为：
    /// - 直接操作系统寄存器
    /// - 执行特权指令
    /// - 可能影响所有后续的内存访问
    ///
    /// ## 使用场景
    ///
    /// - 系统初始化时激活内核地址空间
    /// - 进程切换时激活用户地址空间
    /// - 从用户态返回内核态时恢复内核地址空间
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 激活内核地址空间
    /// let kernel_space = KERNEL_SPACE.exclusive_access();
    /// kernel_space.activate();
    ///
    /// // 激活用户地址空间
    /// let user_space = task.get_user_space();
    /// user_space.activate();
    /// ```
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            asm!("sfence.vma");
        }
    }

    /// 转换虚拟页号到页表项
    ///
    /// 通过地址空间的页表将虚拟页号转换为对应的页表项。
    /// 返回的页表项包含物理页号和权限信息。
    ///
    /// ## Arguments
    ///
    /// * `vpn` - 要转换的虚拟页号
    ///
    /// ## Returns
    ///
    /// - `Some(PageTableEntry)` - 找到对应的页表项
    /// - `None` - 虚拟页号未被映射
    ///
    /// ## 转换过程
    ///
    /// 1. 从虚拟页号提取三级页表索引
    /// 2. 从根页表开始逐级遍历
    /// 3. 检查每级页表项的有效性
    /// 4. 返回最终的叶子页表项
    ///
    /// ## 使用场景
    ///
    /// - 手动地址转换和验证
    /// - 内存访问权限检查
    /// - 调试和诊断工具
    /// - 页面故障处理
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let vpn = VirtPageNum(0x10000);
    /// if let Some(pte) = memory_set.translate(vpn) {
    ///     let ppn = pte.ppn();
    ///     let readable = pte.readable();
    ///     println!("VPN {:#x} -> PPN {:#x}, readable: {}", vpn.0, ppn.0, readable);
    /// } else {
    ///     println!("VPN {:#x} not mapped", vpn.0);
    /// }
    /// ```
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }

    /// 缩小地址空间中的内存区域
    ///
    /// 查找以指定地址开始的内存区域，并将其缩小到新的结束地址。
    /// 主要用于动态内存管理和堆空间回收。
    ///
    /// ## Arguments
    ///
    /// * `start` - 目标区域的起始虚拟地址
    /// * `new_end` - 新的结束虚拟地址（不包含）
    ///
    /// ## Returns
    ///
    /// - `true` - 成功找到并缩小了目标区域
    /// - `false` - 未找到以指定地址开始的区域
    ///
    /// ## 操作过程
    ///
    /// 1. **查找区域**: 遍历 `areas` 列表找到起始地址匹配的区域
    /// 2. **缩小区域**: 调用区域的 `shrink_to()` 方法
    /// 3. **清理资源**: 被移除的页面自动释放对应的物理页帧
    ///
    /// ## 地址对齐
    ///
    /// - `start` 会向下对齐到页边界进行区域匹配
    /// - `new_end` 会向上对齐到页边界确定新边界
    ///
    /// ## 使用场景
    ///
    /// - 堆空间收缩（`sbrk` 系统调用）
    /// - 内存回收和优化
    /// - 动态库卸载
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 缩小堆空间
    /// let heap_start = VirtAddr::from(0x10000000);
    /// let new_heap_end = VirtAddr::from(0x10008000); // 从 64KB 缩小到 32KB
    ///
    /// if memory_set.shrink_to(heap_start, new_heap_end) {
    ///     println!("Heap shrunk successfully");
    /// } else {
    ///     println!("Heap region not found");
    /// }
    /// ```
    #[allow(unused)]
    pub fn shrink_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.shrink_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// 扩展地址空间中的内存区域
    ///
    /// 查找以指定地址开始的内存区域，并将其扩展到新的结束地址。
    /// 主要用于动态内存分配和堆空间扩展。
    ///
    /// ## Arguments
    ///
    /// * `start` - 目标区域的起始虚拟地址
    /// * `new_end` - 新的结束虚拟地址（不包含）
    ///
    /// ## Returns
    ///
    /// - `true` - 成功找到并扩展了目标区域
    /// - `false` - 未找到以指定地址开始的区域
    ///
    /// ## 操作过程
    ///
    /// 1. **查找区域**: 遍历 `areas` 列表找到起始地址匹配的区域
    /// 2. **扩展区域**: 调用区域的 `append_to()` 方法
    /// 3. **分配资源**: 为新增的页面分配物理页帧
    ///
    /// ## 地址对齐
    ///
    /// - `start` 会向下对齐到页边界进行区域匹配
    /// - `new_end` 会向上对齐到页边界确定新边界
    ///
    /// ## 内存分配
    ///
    /// 对于 Framed 映射的区域，每个新增的页面都会分配一个独立的
    /// 物理页帧，实现完整的地址空间隔离。
    ///
    /// ## 使用场景
    ///
    /// - 堆空间扩展（`sbrk` 系统调用）
    /// - 动态库加载
    /// - 用户栈扩展
    /// - 内存映射文件扩展
    ///
    /// ## Panics
    ///
    /// 如果物理页帧分配失败（内存不足）
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 扩展堆空间
    /// let heap_start = VirtAddr::from(0x10000000);
    /// let new_heap_end = VirtAddr::from(0x10010000); // 从 32KB 扩展到 64KB
    ///
    /// if memory_set.append_to(heap_start, new_heap_end) {
    ///     println!("Heap expanded successfully");
    /// } else {
    ///     println!("Heap region not found");
    /// }
    /// ```
    #[allow(unused)]
    pub fn append_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.append_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// 获取地址空间的页表标识符
    ///
    /// 返回当前地址空间的页表标识符（`satp` 寄存器值），用于地址空间切换
    /// 和页表管理。该标识符包含页表模式、ASID 和根页表物理页号信息。
    ///
    /// ## Returns
    ///
    /// `usize` - 页表标识符，可直接写入 `satp` 寄存器
    ///
    /// ## satp 寄存器格式
    ///
    /// ```text
    /// ┌─────────────┬─────────────────┬─────────────────────────────────────────────┐
    /// │    MODE     │      ASID       │                    PPN                      │
    /// │   (4bit)    │     (16bit)     │                  (44bit)                    │
    /// └─────────────┴─────────────────┴─────────────────────────────────────────────┘
    /// ```
    ///
    /// ## 使用场景
    ///
    /// - **地址空间切换**: 进程切换时设置新的页表
    /// - **陷阱上下文**: 保存当前地址空间标识符
    /// - **调试和监控**: 获取当前活跃的页表信息
    ///
    /// ## 与 `activate()` 的关系
    ///
    /// `token()` 获取标识符，`activate()` 使用标识符：
    /// ```rust
    /// let token = memory_set.token();  // 获取标识符
    /// // ... 可以保存 token 供后续使用
    /// memory_set.activate();           // 激活地址空间（内部调用 token()）
    /// ```
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 获取内核地址空间标识符
    /// let kernel_token = KERNEL_SPACE.exclusive_access().token();
    ///
    /// // 在任务切换中使用
    /// let user_token = task.memory_set.token();
    /// // 保存到陷阱上下文中...
    /// ```
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
}

/// 重映射测试函数
///
/// 验证内核地址空间中各个段的内存权限设置是否正确。通过检查内核的
/// .text、.rodata 和 .data 段的中间地址的权限，确保内存保护机制正常工作。
///
/// ## 测试内容
///
/// 1. **代码段权限测试** (.text)
///    - 验证代码段不可写（只读+可执行）
///    - 防止代码被恶意修改
///
/// 2. **只读数据段权限测试** (.rodata)  
///    - 验证只读数据段不可写
///    - 保护常量数据不被修改
///
/// 3. **数据段权限测试** (.data)
///    - 验证数据段不可执行
///    - 防止数据被当作代码执行（NX 保护）
///
/// ## 测试方法
///
/// - 计算每个段的中间地址作为测试点
/// - 通过页表转换获取对应的页表项
/// - 检查页表项的权限标志位
///
/// ## 安全意义
///
/// 这些权限检查实现了重要的安全机制：
/// - **W^X 原则**: 内存页面要么可写，要么可执行，不能同时具备
/// - **代码完整性**: 防止代码段被恶意修改
/// - **数据执行保护**: 防止缓冲区溢出等攻击
///
/// ## Panics
///
/// 如果任何权限检查失败，函数会触发 panic，表明内存保护机制存在问题
///
/// ## 使用场景
///
/// - 系统初始化后的自检
/// - 调试内存权限配置
/// - 验证页表设置的正确性
///
/// ## Examples
///
/// ```rust
/// // 通常在内核初始化完成后调用
/// remap_test(); // 如果通过，打印 "remap_test passed!"
/// ```
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.exclusive_access();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert_eq!(
        kernel_space
            .page_table
            .translate(mid_text.floor())
            .unwrap()
            .writable(),
        false
    );
    assert_eq!(
        kernel_space
            .page_table
            .translate(mid_rodata.floor())
            .unwrap()
            .writable(),
        false,
    );
    assert_eq!(
        kernel_space
            .page_table
            .translate(mid_data.floor())
            .unwrap()
            .executable(),
        false,
    );
    println!("remap_test passed!");
}
