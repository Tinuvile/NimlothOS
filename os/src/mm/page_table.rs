//! # 页表管理模块
//!
//! 提供 RISC-V SV39 分页机制的页表实现，支持三级页表的创建、管理和地址转换功能。
//! 实现了页表项的标志位管理、页表的构建与查询，以及虚拟地址到物理地址的转换。
//!
//! ## 核心组件
//!
//! - [`PTEFlags`] - 页表项标志位，控制页面的访问权限和属性
//! - [`PageTableEntry`] - 页表项，存储物理页号和标志位
//! - [`PageTable`] - 页表结构，管理三级页表的层次结构
//! - [`translate_byte_buffer()`] - 跨页面缓冲区地址转换
//!
//! ## SV39 分页机制
//!
//! RISC-V SV39 使用 39 位虚拟地址和三级页表结构：
//!
//! ```text
//! 虚拟地址格式 (39位):
//! ┌─────────────┬─────────────┬─────────────┬───────────────┐
//! │   VPN[2]    │   VPN[1]    │   VPN[0]    │    Offset     │
//! │   (9bit)    │   (9bit)    │   (9bit)    │    (12bit)    │
//! └─────────────┴─────────────┴─────────────┴───────────────┘
//!
//! 页表遍历过程:
//! 1. 使用 VPN[2] 在一级页表中查找
//! 2. 使用 VPN[1] 在二级页表中查找  
//! 3. 使用 VPN[0] 在三级页表中查找
//! 4. 获得物理页号，加上页内偏移得到物理地址
//! ```
//!
//! ## 页表项标志位
//!
//! | Flag | Name | Description |
//! |------|------|-------------|
//! | V | Valid | Page table entry is valid |
//! | R | Read | Readable permission |
//! | W | Write | Writable permission |
//! | X | Execute | Executable permission |
//! | U | User | User mode accessible |
//! | G | Global | Global page |
//! | A | Accessed | Page has been accessed |
//! | D | Dirty | Page has been modified |
//!
//! ## 使用示例
//!
//! ```rust
//! // 创建新的页表
//! let mut page_table = PageTable::new();
//!
//! // 建立映射关系
//! let vpn = VirtAddr::from(0x10000000).floor();
//! let ppn = PhysPageNum(0x80200);
//! let flags = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::V;
//! page_table.map(vpn, ppn, flags);
//!
//! // 地址转换
//! if let Some(pte) = page_table.translate(vpn) {
//!     let physical_addr = pte.ppn();
//!     println!("虚拟页 {:?} 映射到物理页 {:?}", vpn, physical_addr);
//! }
//!
//! // 获取页表令牌用于硬件
//! let token = page_table.token();
//! ```
//!
//! ## 内存管理
//!
//! 页表通过 [`FrameTracker`] 进行 RAII 管理：
//! - 页表创建时自动分配页帧
//! - 页表销毁时自动释放所有相关页帧
//! - 中间页表按需创建和管理
//!
//! ## 硬件集成
//!
//! - 支持 RISC-V satp 寄存器格式
//! - 兼容硬件页表遍历机制
//! - 提供高效的地址转换功能

use crate::mm::{
    PhysAddr, PhysPageNum, VirtAddr, VirtPageNum,
    address::StepByOne,
    frame_allocator::{FrameTracker, frame_alloc},
};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// 页表项标志位 (Page Table Entry Flags)
    ///
    /// 控制页表项的访问权限和属性，基于 RISC-V 特权级架构规范定义。
    /// 这些标志位存储在页表项的低 8 位，用于硬件 MMU 的权限检查和页面管理。
    ///
    /// ## 标志位详细说明
    ///
    /// - **V (Valid)**: 页表项有效位，为 0 时表示页面未映射
    /// - **R (Read)**: 读权限，允许从页面读取数据
    /// - **W (Write)**: 写权限，允许向页面写入数据
    /// - **X (Execute)**: 执行权限，允许从页面获取指令
    /// - **U (User)**: 用户权限，用户态可访问该页面
    /// - **G (Global)**: 全局页面，在地址空间切换时不会被刷新
    /// - **A (Accessed)**: 访问位，硬件在访问页面时设置
    /// - **D (Dirty)**: 脏页位，硬件在写入页面时设置
    ///
    /// ## 权限组合示例
    ///
    /// ```rust
    /// // 只读代码页
    /// let code_flags = PTEFlags::V | PTEFlags::R | PTEFlags::X;
    ///
    /// // 可读写数据页
    /// let data_flags = PTEFlags::V | PTEFlags::R | PTEFlags::W;
    ///
    /// // 用户可访问的页面
    /// let user_flags = PTEFlags::V | PTEFlags::R | PTEFlags::W | PTEFlags::U;
    /// ```
    ///
    /// ## RISC-V 规范参考
    ///
    /// 详细定义请参考 [RISC-V 特权级架构手册 - 地址保护](https://five-embeddev.com/riscv-priv-isa-manual/Priv-v1.12/supervisor.html#addressing-and-memory-protection)
    #[derive(PartialEq, Eq)]
    pub struct PTEFlags: u8 {
        /// Valid - 页表项有效，为 1 表示映射有效
        const V = 1 << 0;
        /// Read - 读权限，为 1 表示可读
        const R = 1 << 1;
        /// Write - 写权限，为 1 表示可写
        const W = 1 << 2;
        /// Execute - 执行权限，为 1 表示可执行指令
        const X = 1 << 3;
        /// User - 用户权限，为 1 表示用户态可访问
        const U = 1 << 4;
        /// Global - 全局页面，为 1 表示全局可见
        const G = 1 << 5;
        /// Accessed - 访问位，硬件设置表示页面已被访问
        const A = 1 << 6;
        /// Dirty - 脏页位，硬件设置表示页面已被修改
        const D = 1 << 7;
    }
}

/// 页表项 (Page Table Entry)
///
/// 表示页表中的单个条目，包含物理页号和访问权限标志位。
/// 在 RISC-V SV39 中，每个页表项占用 64 位（8字节）。
///
/// ## 位域结构
///
/// ```text
/// 63        54 53       10 9        8 7      0
/// ┌───────────┬───────────┬────-─────┬────────┐
/// │ reserved  │    PPN    │ reserved │ flags  │
/// └───────────┴───────────┴─────────-┴────────┘
/// ```
///
/// - **标志位 [7:0]**: [`PTEFlags`] 定义的访问权限和属性
/// - **物理页号 [53:10]**: 指向实际的物理页面
/// - **保留位**: 未使用，必须为 0
///
/// ## 主要用途
///
/// - 存储虚拟页到物理页的映射关系
/// - 控制页面的访问权限（读/写/执行）
/// - 标记页面状态（已访问/已修改）
/// - 区分用户页面和内核页面
///
/// ## 使用示例
///
/// ```rust
/// // 创建映射到物理页的页表项
/// let ppn = PhysPageNum(0x80200);
/// let flags = PTEFlags::V | PTEFlags::R | PTEFlags::W;
/// let pte = PageTableEntry::new(ppn, flags);
///
/// // 检查权限
/// assert!(pte.is_valid());
/// assert!(pte.readable());
/// assert!(pte.writable());
/// assert!(!pte.executable());
///
/// // 提取物理页号
/// let mapped_ppn = pte.ppn();
/// ```
#[repr(C)]
#[derive(Copy, Clone)]
pub struct PageTableEntry {
    /// 页表项的原始位表示
    ///
    /// 包含物理页号（位 53:10）和标志位（位 7:0）的完整编码。
    /// 直接对应硬件页表项的格式，可以被 MMU 硬件解析。
    pub bits: usize,
}

impl PageTableEntry {
    /// 创建新的页表项
    ///
    /// 根据物理页号和标志位创建一个新的页表项，自动进行位域编码。
    ///
    /// ## Arguments
    ///
    /// * `ppn` - 目标物理页号，指向要映射到的物理页面
    /// * `flags` - 页面访问权限和属性标志位
    ///
    /// ## Returns
    ///
    /// 返回编码后的页表项，可以直接写入页表
    ///
    /// ## 编码格式
    ///
    /// 页表项的位域编码：
    /// - `bits = (ppn << 10) | flags`
    /// - 物理页号占用位 53:10
    /// - 标志位占用位 7:0
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let ppn = PhysPageNum(0x80200);
    /// let flags = PTEFlags::V | PTEFlags::R | PTEFlags::W;
    /// let pte = PageTableEntry::new(ppn, flags);
    /// ```
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits() as usize,
        }
    }

    /// 创建空的页表项
    ///
    /// 创建一个所有位都为 0 的页表项，表示未映射的页面。
    /// 空页表项的 Valid 位为 0，MMU 会将其视为无效映射。
    ///
    /// ## Returns
    ///
    /// 返回无效的空页表项
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let empty_pte = PageTableEntry::empty();
    /// assert!(!empty_pte.is_valid());
    /// ```
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }

    /// 提取物理页号
    ///
    /// 从页表项中解码出物理页号，用于地址转换。
    ///
    /// ## Returns
    ///
    /// 返回页表项中编码的物理页号
    ///
    /// ## 解码过程
    ///
    /// 从位 53:10 提取 44 位物理页号：
    /// ```text
    /// ppn = (bits >> 10) & ((1 << 44) - 1)
    /// ```
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let original_ppn = PhysPageNum(0x80200);
    /// let pte = PageTableEntry::new(original_ppn, PTEFlags::V);
    /// let extracted_ppn = pte.ppn();
    /// assert_eq!(original_ppn, extracted_ppn);
    /// ```
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }

    /// 提取标志位
    ///
    /// 从页表项中解码出访问权限和属性标志位。
    ///
    /// ## Returns
    ///
    /// 返回页表项中的 [`PTEFlags`] 标志位
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let flags = PTEFlags::V | PTEFlags::R | PTEFlags::W;
    /// let pte = PageTableEntry::new(PhysPageNum(0x80200), flags);
    /// let extracted_flags = pte.flags();
    /// assert_eq!(flags, extracted_flags);
    /// ```
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }

    /// 检查页表项是否有效
    ///
    /// 检查 Valid (V) 标志位，判断该页表项是否表示有效的映射。
    /// 无效页表项会导致硬件产生页面错误异常。
    ///
    /// ## Returns
    ///
    /// - `true` - 页表项有效，表示有效映射
    /// - `false` - 页表项无效，表示未映射页面
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let valid_pte = PageTableEntry::new(PhysPageNum(0x80200), PTEFlags::V);
    /// assert!(valid_pte.is_valid());
    ///
    /// let invalid_pte = PageTableEntry::empty();
    /// assert!(!invalid_pte.is_valid());
    /// ```
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }

    /// 检查页面是否可读
    ///
    /// 检查 Read (R) 标志位，判断该页面是否允许读取操作。
    ///
    /// ## Returns
    ///
    /// - `true` - 页面可读
    /// - `false` - 页面不可读，读取会触发访问异常
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }

    /// 检查页面是否可写
    ///
    /// 检查 Write (W) 标志位，判断该页面是否允许写入操作。
    ///
    /// ## Returns
    ///
    /// - `true` - 页面可写
    /// - `false` - 页面不可写，写入会触发访问异常
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }

    /// 检查页面是否可执行
    ///
    /// 检查 Execute (X) 标志位，判断该页面是否允许执行指令。
    ///
    /// ## Returns
    ///
    /// - `true` - 页面可执行指令
    /// - `false` - 页面不可执行，取指会触发访问异常
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// 页表 (Page Table)
///
/// 管理三级页表结构，实现虚拟地址到物理地址的映射机制。
/// 采用 RISC-V SV39 分页方案，支持页面的映射、取消映射和地址转换。
///
/// ## 结构组成
///
/// - **根页表**: 一级页表的物理页号，用于硬件 MMU 查找
/// - **页帧管理**: 自动管理所有页表页帧的生命周期
///
/// ## 三级页表结构
///
/// ```text
/// Address translation process:
///              VPN[2]        VPN[1]          VPN[0]      Offset
///              (9bit)        (9bit)          (9bit)      (12bit)
///                 │            │              │           │
///                 v            │              │           │
///         ┌─────────────┐      │              │           │
///    satp │ Level1 PPN  │      │              │           │
///         └─────────────┘      │              │           │
///                 │            │              │           │
///                 v            v              │           │
///         ┌─────────────┐ ┌─────────────┐     │           │
///         │ Level2 PPN  │ │ Level2 PPN  │     │           │
///         └─────────────┘ └─────────────┘     │           │
///                              │              │           │
///                              v              v           │
///                      ┌─────────────┐ ┌─────────────┐    │
///                      │ Level3 PPN  │ │ Level3 PPN  │    │
///                      └─────────────┘ └─────────────┘    │
///                               │             │           │
///                               v             v           v
///                       ┌─────────────┐ ┌─────────────┬──────┐
///                       │  Data PPN   │ │  Data PPN   │Offset│
///                       └─────────────┘ └─────────────┴──────┘
///                              │               │
///                              v               v
///                        Physical Addr   Final Physical Addr
/// ```
///
/// ## RAII 内存管理
///
/// 页表通过 RAII 机制自动管理所有页表页帧：
/// - 创建页表时分配根页表页帧
/// - 添加映射时按需分配中间页表页帧
/// - 销毁页表时自动释放所有页表页帧
///
/// ## 使用示例
///
/// ```rust
/// // 创建新页表
/// let mut page_table = PageTable::new();
///
/// // 建立虚拟页到物理页的映射
/// let vpn = VirtAddr::from(0x1000_0000).floor();
/// let ppn = PhysPageNum(0x80200);
/// let flags = PTEFlags::V | PTEFlags::R | PTEFlags::W | PTEFlags::U;
/// page_table.map(vpn, ppn, flags);
///
/// // 查询地址转换
/// if let Some(pte) = page_table.translate(vpn) {
///     println!("虚拟页 {:?} 映射到 {:?}", vpn, pte.ppn());
/// }
///
/// // 获取页表令牌供硬件使用
/// let satp_value = page_table.token();
/// ```
pub struct PageTable {
    /// 根页表的物理页号
    ///
    /// 指向一级页表的物理页面，用于硬件 MMU 开始页表遍历。
    /// 该值会被编码到 satp 寄存器中供硬件使用。
    root_ppn: PhysPageNum,

    /// 页表页帧追踪器列表  
    ///
    /// 存储所有页表结构占用的物理页帧，包括根页表和所有中间页表。
    /// 通过 RAII 机制确保页表销毁时自动释放所有页帧。
    frames: Vec<FrameTracker>,
}

impl PageTable {
    /// 创建新的页表
    ///
    /// 分配根页表页帧并初始化页表结构，创建一个空的三级页表。
    /// 新创建的页表没有任何映射，所有虚拟地址都未映射。
    ///
    /// ## Returns
    ///
    /// 返回新创建的页表实例
    ///
    /// ## 初始化过程
    ///
    /// 1. 分配一个物理页帧作为根页表
    /// 2. 清零页表内容（由 [`FrameTracker::new()`] 完成）
    /// 3. 将页帧添加到 RAII 管理列表
    ///
    /// ## Panics
    ///
    /// 如果物理内存不足导致页帧分配失败会触发 panic
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let page_table = PageTable::new();
    /// // 此时页表为空，所有地址转换都会失败
    /// ```
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }

    /// 查找页表项（按需创建中间页表）
    ///
    /// 在三级页表中查找指定虚拟页号对应的页表项，如果中间页表不存在则自动创建。
    /// 主要用于建立新映射时的页表项查找。
    ///
    /// ## Arguments
    ///
    /// * `vpn` - 要查找的虚拟页号
    ///
    /// ## Returns
    ///
    /// 返回指向目标页表项的可变引用，如果创建失败返回 `None`
    ///
    /// ## 查找过程
    ///
    /// 1. 解析虚拟页号为三级页表索引
    /// 2. 从根页表开始逐级查找
    /// 3. 如果中间页表项无效，分配新页帧并创建页表
    /// 4. 返回三级页表中的目标页表项
    ///
    /// ## 自动创建机制
    ///
    /// 当遇到无效的中间页表项时：
    /// - 分配新的物理页帧作为下级页表
    /// - 设置页表项指向新页表，标志位为 V（仅有效）
    /// - 将新页帧加入 RAII 管理列表
    ///
    /// ## Panics
    ///
    /// 如果页帧分配失败会触发 panic
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for i in 0..3 {
            let pte = &mut ppn.get_pte_array()[idxs[i]];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }

    /// 查找页表项（只读查找）
    ///
    /// 在三级页表中查找指定虚拟页号对应的页表项，不会创建任何中间页表。
    /// 主要用于地址转换和映射状态查询。
    ///
    /// ## Arguments
    ///
    /// * `vpn` - 要查找的虚拟页号
    ///
    /// ## Returns
    ///
    /// - `Some(&mut PageTableEntry)` - 找到有效的页表项
    /// - `None` - 页表项不存在或中间页表无效
    ///
    /// ## 查找过程
    ///
    /// 1. 解析虚拟页号为三级页表索引
    /// 2. 从根页表开始逐级查找
    /// 3. 如果任何中间页表项无效，立即返回 `None`
    /// 4. 返回三级页表中的目标页表项
    ///
    /// ## 与 `find_pte_create` 的区别
    ///
    /// - 本方法只查找，不创建
    /// - 用于只读操作，不修改页表结构
    /// - 性能更高，不涉及内存分配
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for i in 0..3 {
            let pte = &mut ppn.get_pte_array()[idxs[i]];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }

    /// 建立虚拟页到物理页的映射
    ///
    /// 在页表中创建从虚拟页号到物理页号的映射关系，并设置相应的访问权限。
    /// 如果必要的中间页表不存在，会自动创建。
    ///
    /// ## Arguments
    ///
    /// * `vpn` - 要映射的虚拟页号
    /// * `ppn` - 目标物理页号
    /// * `flags` - 页面访问权限标志位（自动添加 V 标志）
    ///
    /// ## 映射过程
    ///
    /// 1. 查找或创建到虚拟页的页表项路径
    /// 2. 检查目标页表项当前为无效状态
    /// 3. 设置页表项指向物理页号，并应用权限标志
    /// 4. 自动添加 Valid (V) 标志位使映射生效
    ///
    /// ## Panics
    ///
    /// 在以下情况会触发 panic：
    /// - 页帧分配失败导致中间页表创建失败
    /// - 虚拟页已经被映射（重复映射检查）
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let mut page_table = PageTable::new();
    /// let vpn = VirtAddr::from(0x1000_0000).floor();
    /// let ppn = PhysPageNum(0x80200);
    ///
    /// // 映射为可读写的用户页
    /// let flags = PTEFlags::R | PTEFlags::W | PTEFlags::U;
    /// page_table.map(vpn, ppn, flags);
    /// ```
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }

    /// 取消虚拟页的映射
    ///
    /// 从页表中移除指定虚拟页号的映射关系，使该虚拟地址变为无效。
    /// 不会释放物理页帧，只是取消映射关系。
    ///
    /// ## Arguments
    ///
    /// * `vpn` - 要取消映射的虚拟页号
    ///
    /// ## 取消映射过程
    ///
    /// 1. 查找虚拟页对应的页表项
    /// 2. 检查页表项当前为有效状态
    /// 3. 将页表项设置为空（全零）
    /// 4. 后续访问该虚拟地址会触发页面错误
    ///
    /// ## 注意事项
    ///
    /// - 不会自动释放物理页帧，需要调用者管理
    /// - 不会回收中间页表，即使它们变为空
    /// - 取消映射后应刷新 TLB 以确保硬件缓存同步
    ///
    /// ## Panics
    ///
    /// 在以下情况会触发 panic：
    /// - 虚拟页未被映射（无效的取消映射操作）
    /// - 中间页表路径不存在
    ///
    /// ## Examples
    ///
    /// ```rust
    /// // 假设 vpn 已经被映射
    /// page_table.unmap(vpn);
    /// // 现在访问 vpn 会触发页面错误
    /// ```
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is not mapped", vpn);
        *pte = PageTableEntry::empty();
    }

    /// 从 satp 寄存器值创建页表
    ///
    /// 根据 RISC-V satp (Supervisor Address Translation and Protection) 寄存器的值
    /// 创建页表实例，主要用于访问当前硬件正在使用的页表。
    ///
    /// ## Arguments
    ///
    /// * `satp` - satp 寄存器的值，包含分页模式和根页表地址
    ///
    /// ## Returns
    ///
    /// 返回基于现有页表的页表实例
    ///
    /// ## satp 寄存器格式
    ///
    /// ```text
    /// 63      60 59           44 43                    0
    /// ┌─────────┬──────────────┬─────────────────────────┐
    /// │  MODE   │     ASID     │         PPN             │
    /// │ (4bit)  │   (16bit)    │       (44bit)           │
    /// └─────────┴──────────────┴─────────────────────────┘
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 创建的页表实例不管理页帧（`frames` 为空）
    /// - 主要用于地址转换，不能用于映射操作
    /// - 假设页表结构已经由其他方式管理
    ///
    /// ## 参考资料
    ///
    /// 详细的 satp 寄存器定义请参考 [RISC-V satp 寄存器](https://five-embeddev.com/riscv-priv-isa-manual/Priv-v1.12/supervisor.html#sec:satp)
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }

    /// 执行虚拟地址转换
    ///
    /// 查找指定虚拟页号的映射，返回对应的页表项副本。
    /// 用于地址转换和权限检查。
    ///
    /// ## Arguments
    ///
    /// * `vpn` - 要转换的虚拟页号
    ///
    /// ## Returns
    ///
    /// - `Some(PageTableEntry)` - 找到有效映射，返回页表项副本
    /// - `None` - 虚拟页未映射或中间页表无效
    ///
    /// ## 转换过程
    ///
    /// 1. 通过三级页表查找页表项
    /// 2. 如果找到有效页表项，返回其副本
    /// 3. 如果路径中任何页表无效，返回 `None`
    ///
    /// ## 使用场景
    ///
    /// - 虚拟地址到物理地址转换
    /// - 检查页面映射状态和权限
    /// - 实现内存管理单元的软件模拟
    ///
    /// ## Examples
    ///
    /// ```rust
    /// if let Some(pte) = page_table.translate(vpn) {
    ///     let ppn = pte.ppn();
    ///     let physical_addr = PhysAddr::from(ppn) + offset;
    ///     println!("虚拟地址转换为物理地址: {:?}", physical_addr);
    /// }
    /// ```
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| pte.clone())
    }

    /// 获取页表令牌
    ///
    /// 生成用于 satp 寄存器的令牌值，包含分页模式和根页表物理页号。
    /// 硬件使用此值进行地址转换。
    ///
    /// ## Returns
    ///
    /// 返回格式化的 satp 寄存器值
    ///
    /// ## 令牌格式
    ///
    /// ```text
    /// 63      60 59           44 43                    0
    /// ┌─────────┬──────────────┬─────────────────────────┐
    /// │    8    │      0       │      root_ppn.0         │
    /// │ (SV39)  │   (ASID)     │   (Root Page PPN)       │
    /// └─────────┴──────────────┴─────────────────────────┘
    /// ```
    ///
    /// ## 模式说明
    ///
    /// - MODE = 8: 表示 SV39 分页模式
    /// - ASID = 0: 地址空间标识符，暂时使用 0
    /// - PPN: 根页表的物理页号
    ///
    /// 参考：[satp寄存器](https://five-embeddev.com/riscv-priv-isa-manual/Priv-v1.12/supervisor.html#sec:satp)
    ///
    /// ## 使用方式
    ///
    /// ```rust
    /// let token = page_table.token();
    /// // 将 token 写入 satp 寄存器激活页表
    /// unsafe {
    ///     asm!("csrw satp, {}", in(reg) token);
    ///     // 刷新 TLB
    ///     asm!("sfence.vma");
    /// }
    /// ```
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0 // TODO：ASID暂时没有加，进程的时候再加
    }

    /// 执行虚拟地址到物理地址的转换（带页内偏移）
    ///
    /// 在当前页表下，将给定的虚拟地址转换为对应的物理地址。
    /// 与 [`translate`] 返回页表项不同，本函数会将页内偏移合并到
    /// 最终的物理地址中，返回可直接用于内存访问的物理地址。
    ///
    /// ## Arguments
    ///
    /// * `va` - 需要转换的虚拟地址（包含页内偏移）
    ///
    /// ## Returns
    ///
    /// - `Some(PhysAddr)` - 转换成功，返回完整物理地址
    /// - `None` - 虚拟地址未映射或中间页表无效
    ///
    /// ## 转换过程
    ///
    /// 1. 取出虚拟地址的页内偏移 `offset`
    /// 2. 通过 `translate()` 查找页表项获取对齐的物理页起始地址
    /// 3. 将页内偏移加到对齐地址上形成最终物理地址
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let va = VirtAddr::from(0x1000_0123);
    /// if let Some(pa) = page_table.translate_va(va) {
    ///     // pa = 对齐物理页起始地址 + 0x123
    /// }
    /// ```
    pub fn translate_va(&self, va: VirtAddr) -> Option<PhysAddr> {
        self.find_pte(va.clone().floor()).map(|pte| {
            let aligned_pa: PhysAddr = pte.ppn().into();
            let offset = va.page_offset();
            let aligned_pa_usize: usize = aligned_pa.into();
            (aligned_pa_usize + offset).into()
        })
    }
}

/// 转换跨页面字节缓冲区
///
/// 将一个可能跨越多个页面的虚拟地址缓冲区转换为对应的物理页面切片列表。
/// 主要用于系统调用中访问用户态传递的缓冲区数据。
///
/// ## Arguments
///
/// * `token` - 页表令牌（satp 寄存器值），指定要使用的页表
/// * `ptr` - 缓冲区起始虚拟地址指针
/// * `len` - 缓冲区长度（字节数）
///
/// ## Returns
///
/// 返回物理页面切片的向量，每个切片对应缓冲区在一个物理页面中的部分
///
/// ## 转换过程
///
/// 1. 根据页表令牌创建页表实例
/// 2. 按页面边界分割缓冲区范围
/// 3. 逐个转换每个页面的虚拟地址到物理地址
/// 4. 返回每个页面中对应部分的可变切片
///
/// ## 跨页面处理
///
/// ```text
/// Virtual Buffer:  [████████████████████████████]
///                    │            │           │
/// Page Boundary:  ┌──┴──┐    ┌────┴────┐   ┌──┴──┐
/// Physical Page:  Page A     Page B       Page C
/// Return Slice:   slice[0]   slice[1]     slice[2]
/// ```
///
/// ## 使用场景
///
/// - 系统调用中访问用户态缓冲区
/// - DMA 操作的地址转换
/// - 跨页面数据的零拷贝访问
///
/// ## Safety
///
/// 返回的切片具有 `'static` 生命周期，调用者需要确保：
/// - 页表在使用期间保持有效
/// - 物理页面没有被其他代码同时修改
/// - 虚拟地址确实已正确映射
///
/// ## Panics
///
/// 如果缓冲区中任何页面未被映射会触发 panic
///
/// ## Examples
///
/// ```rust
/// // 转换用户态缓冲区以供内核访问
/// let user_buffer_ptr = 0x10001000 as *const u8;
/// let len = 8192; // 跨越 3 个页面
/// let token = current_user_token();
///
/// let slices = translate_byte_buffer(token, user_buffer_ptr, len);
/// for slice in slices {
///     // 访问每个物理页面中的数据
///     process_data(slice);
/// }
/// ```
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if start_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

/// 转换以 0 结尾的用户字符串为内核字符串
///
/// 在给定页表令牌（satp 值）下，从用户虚拟地址空间读取
/// 以 `\0` 结尾的字节序列，并构造一个内核态的 [`String`] 返回。
/// 该函数会按字节跨页面地读取，直到遇到终止符 `\0` 为止。
///
/// ## Arguments
///
/// * `token` - 页表令牌（satp 寄存器值），指定用户地址空间
/// * `ptr` - 用户空间的 C 风格字符串起始指针
///
/// ## Returns
///
/// - 返回对应内容的内核态 [`String`]
///
/// ## 行为特征
///
/// - 支持跨页面读取
/// - 逐字节读取直到遇到 `\0`
/// - 如果中途地址未映射将触发 `unwrap()` panic（未来可改为错误返回）
///
/// ## Safety
///
/// - 假设 `ptr` 指向的是有效的、可读的用户地址
/// - 调用者需确保字符串以 `\0` 终止，否则会一直向后读取直到出错
///
/// ## Examples
///
/// ```rust
/// // 在系统调用中，将用户提供的路径参数转换为内核字符串
/// let path = translated_str(current_user_token(), user_ptr);
/// println!("open path: {}", path);
/// ```
pub fn translated_str(token: usize, ptr: *const u8) -> String {
    let page_table = PageTable::from_token(token);
    let mut string = String::new();
    let mut va = ptr as usize;
    loop {
        let ch: u8 = *(page_table
            .translate_va(VirtAddr::from(va))
            .unwrap()
            .get_mut());
        if ch == 0 {
            break;
        } else {
            string.push(ch as char);
            va += 1;
        }
    }
    string
}

/// 将用户虚拟地址转换为内核可写引用
///
/// 在给定的页表令牌（satp 值）下，将用户空间的指针转换为
/// 内核态可写引用，便于在系统调用中直接修改用户缓冲区数据。
///
/// ## Type Parameters
///
/// * `T` - 目标引用的数据类型
///
/// ## Arguments
///
/// * `token` - 页表令牌（satp 寄存器值）
/// * `ptr` - 用户空间指针
///
/// ## Returns
///
/// - `'static mut T` - 指向用户内存的可写引用（生命周期由调用者语义保证）
///
/// ## Safety
///
/// - 假设 `ptr` 指向有效且可写的用户内存
/// - 调用者需确保不存在别名冲突与数据竞争
/// - 如果地址未映射将触发 `unwrap()` panic（未来可改为错误返回）
///
/// ## Examples
///
/// ```rust
/// // 将系统调用的返回值写回到用户缓冲区
/// let buf: &mut u8 = translated_refmut(user_token, user_ptr);
/// *buf = value as u8;
/// ```
pub fn translated_refmut<T>(token: usize, ptr: *mut T) -> &'static mut T {
    let page_table = PageTable::from_token(token);
    let va = ptr as usize;
    page_table
        .translate_va(VirtAddr::from(va))
        .unwrap()
        .get_mut()
}
