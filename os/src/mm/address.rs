//! # 内存地址管理模块
//!
//! 提供虚拟地址和物理地址的类型安全封装，以及地址与页号之间的转换功能。
//! 支持 RISC-V SV39 分页机制，实现地址对齐、页表操作等核心内存管理功能。
//!
//! ## 主要类型
//!
//! - [`PhysAddr`] - 物理地址，56 位有效
//! - [`VirtAddr`] - 虚拟地址，39 位有效，支持符号扩展
//! - [`PhysPageNum`] - 物理页号，用于物理页帧管理
//! - [`VirtPageNum`] - 虚拟页号，用于虚拟页管理和页表索引
//!
//! ## 核心功能
//!
//! - **地址对齐**: 提供向上/向下页对齐功能，确保内存管理的边界安全
//! - **类型转换**: 安全的地址与页号、不同地址类型间的转换
//! - **页表操作**: 通过物理页号直接访问页表项和页面数据
//! - **范围迭代**: 支持虚拟页号范围的迭代，便于批量页面操作
//!
//! ## SV39 地址格式
//!
//! ```text
//! 虚拟地址 (39位有效):
//! ┌─────────────┬─────────────┬─────────────┬─────────────────┐
//! │   VPN[2]    │   VPN[1]    │   VPN[0]    │     Offset      │
//! │   (9bit)    │   (9bit)    │   (9bit)    │     (12bit)     │
//! └─────────────┴─────────────┴─────────────┴─────────────────┘
//!
//! 物理地址 (56位有效):
//! ┌─────────────────────────────────────────┬─────────────────┐
//! │                 PPN                     │     Offset      │
//! │               (44bit)                   │     (12bit)     │
//! └─────────────────────────────────────────┴─────────────────┘
//! ```
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::mm::{PhysAddr, VirtAddr, PhysPageNum, VirtPageNum};
//!
//! // 地址创建和转换
//! let pa = PhysAddr::from(0x80200000usize);
//! let ppn = pa.floor(); // 向下对齐到页边界
//!
//! // 页号操作
//! let bytes = ppn.get_bytes_array(); // 获取页面数据
//! let ptes = ppn.get_pte_array();    // 获取页表项数组
//!
//! // 虚拟地址页表索引
//! let va = VirtAddr::from(0x10000000usize);
//! let vpn = va.floor();
//! let indices = vpn.indexes(); // 获取三级页表索引
//! ```

use super::PageTableEntry;
use crate::config::{PAGE_SIZE, PAGE_SIZE_BITS};
use core::fmt::{self, Debug, Formatter};

const PA_WIDTH_SV39: usize = 56;
const VA_WIDTH_SV39: usize = 39;
const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;
const VPN_WIDTH_SV39: usize = VA_WIDTH_SV39 - PAGE_SIZE_BITS;

/// 物理地址 (Physical Address)
///
/// 封装 56 位物理地址，支持 RISC-V SV39 分页模式。物理地址直接对应
/// 系统中的物理内存位置，用于硬件级别的内存访问。
///
/// ## 地址范围
///
/// - 有效位数：56 位 (0 - 2^56-1)
/// - 页面大小：4KB (2^12 字节)
/// - 页内偏移：低 12 位
/// - 物理页号：高 44 位
///
/// ## 主要方法
///
/// - [`floor`] - 向下对齐到页边界
/// - [`ceil`] - 向上对齐到页边界  
/// - [`page_offset`] - 获取页内偏移
/// - [`aligned`] - 检查是否页对齐
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);

/// 虚拟地址 (Virtual Address)
///
/// 封装 39 位虚拟地址，支持 RISC-V SV39 分页模式。虚拟地址经过
/// MMU 转换后访问物理内存，提供地址空间隔离和保护。
///
/// ## 地址格式
///
/// - 有效位数：39 位
/// - 符号扩展：高位扩展到 64 位
/// - VPN[2:0]：27 位虚拟页号 (分3级，每级9位)
/// - 页内偏移：低 12 位
///
/// ## 地址空间布局
///
/// - 用户空间：0x0000_0000_0000_0000 - 0x0000_003F_FFFF_FFFF
/// - 内核空间：0xFFFF_FFC0_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);

/// 物理页号 (Physical Page Number)
///
/// 表示 4KB 物理页面的编号，用于物理页帧分配和管理。
/// 物理页号乘以页面大小即可得到页面起始物理地址。
///
/// ## 主要功能
///
/// - 页面数据访问：[`get_bytes_array`]
/// - 页表项访问：[`get_pte_array`]
/// - 任意类型访问：[`get_mut<T>`]
///
/// ## 使用场景
///
/// - 物理内存分配器
/// - 页表管理
/// - DMA 缓冲区管理
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);

/// 虚拟页号 (Virtual Page Number)  
///
/// 表示 4KB 虚拟页面的编号，用于页表查找和虚拟内存管理。
/// 包含三级页表索引信息，支持 SV39 分页机制。
///
/// ## 页表索引结构
///
/// 27 位虚拟页号分为三级索引：
/// - VPN[2]：一级页表索引 (高 9 位)
/// - VPN[1]：二级页表索引 (中 9 位)  
/// - VPN[0]：三级页表索引 (低 9 位)
///
/// ## 主要方法
///
/// - [`indexes`] - 获取三级页表索引数组
/// - [`step`] - 递增页号 (支持范围迭代)
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);

impl PhysAddr {
    /// 向下对齐到页边界
    ///
    /// 将物理地址向下舍入到最接近的页面边界，返回对应的物理页号。
    /// 用于确保地址不会超出页面范围。
    ///
    /// ## Returns
    ///
    /// 返回包含该地址的物理页号
    ///
    /// ## Examples
    ///
    /// ```
    /// let addr = PhysAddr(0x80201234);
    /// let ppn = addr.floor(); // 0x80201000 / 0x1000 = 0x80201
    /// ```
    pub fn floor(&self) -> PhysPageNum {
        PhysPageNum(self.0 / PAGE_SIZE)
    }

    /// 向上对齐到页边界
    ///
    /// 将物理地址向上舍入到最接近的页面边界，返回对应的物理页号。
    /// 用于分配足够的页面空间包含指定地址。
    ///
    /// ## Returns
    ///
    /// 返回能够包含该地址的最小物理页号
    ///
    /// ## Examples
    ///
    /// ```
    /// let addr = PhysAddr(0x80201234);
    /// let ppn = addr.ceil(); // (0x80201234 + 0xfff) / 0x1000 = 0x80202
    /// ```
    ///
    /// ## Special Cases
    ///
    /// 地址为 0 时直接返回页号 0，避免整数下溢
    pub fn ceil(&self) -> PhysPageNum {
        if self.0 == 0 {
            PhysPageNum(0)
        } else {
            PhysPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE)
        }
    }

    /// 获取页内偏移
    ///
    /// 返回地址在页面内的偏移量，范围为 [0, PAGE_SIZE-1]。
    /// 偏移量表示该地址距离页面起始地址的字节数。
    ///
    /// ## Returns
    ///
    /// 页内偏移量 (0-4095)
    ///
    /// ## Examples
    ///
    /// ```
    /// let addr = PhysAddr(0x80201234);
    /// let offset = addr.page_offset(); // 0x234 = 564
    /// ```
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    /// 检查地址是否页对齐
    ///
    /// 判断地址是否正好位于页面边界上（页内偏移为 0）。
    /// 页对齐的地址可以直接用作页面起始地址。
    ///
    /// ## Returns
    ///
    /// 如果地址页对齐返回 `true`，否则返回 `false`
    ///
    /// ## Examples
    ///
    /// ```
    /// assert!(PhysAddr(0x80201000).aligned()); // 页对齐
    /// assert!(!PhysAddr(0x80201234).aligned()); // 非页对齐
    /// ```
    pub fn aligned(&self) -> bool {
        self.page_offset() == 0
    }

    /// 获取指定类型的可变引用
    ///
    /// 将物理地址直接转换为类型 `T` 的可变引用，用于访问该地址处
    /// 存放的内存对象。该方法属于低级原语，绕过了借用检查并依赖
    /// 调用者保证内存安全。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 目标数据类型，必须与地址处的数据布局兼容
    ///
    /// ## Returns
    ///
    /// 指向类型 `T` 的可变引用，生命周期为 `'static`，表示该引用
    /// 在类型系统看来可长期存在（实际由调用者保证其有效期）。
    ///
    /// ## Safety
    ///
    /// 此方法内部使用 `unsafe` 执行裸指针到引用的转换，调用者必须确保：
    /// - 该物理地址可被安全地当作 `*mut T` 访问
    /// - 目标内存已经按 `T` 的布局正确初始化
    /// - 满足 `T` 的对齐要求，否则行为未定义
    /// - 在引用存活期间无数据竞争（不存在其他可变或不可变别名）
    /// - 地址指向的内存在引用存活期间保持有效
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let pa = PhysAddr(0x8020_1000);
    /// let value: &mut u64 = pa.get_mut::<u64>();
    /// *value = 0xdead_beef_dead_beefu64;
    /// ```
    pub fn mut_ref<T>(&self) -> &'static mut T {
        unsafe { (self.0 as *mut T).as_mut().unwrap() }
    }
}

impl VirtAddr {
    /// 向下对齐到页边界
    ///
    /// 将虚拟地址向下舍入到最接近的页面边界，返回对应的虚拟页号。
    /// 在虚拟内存管理中用于确定地址所属的页面。
    ///
    /// ## Returns
    ///
    /// 返回包含该地址的虚拟页号
    pub fn floor(&self) -> VirtPageNum {
        VirtPageNum(self.0 / PAGE_SIZE)
    }

    /// 向上对齐到页边界
    ///
    /// 将虚拟地址向上舍入到最接近的页面边界，返回对应的虚拟页号。
    /// 用于计算映射指定地址范围所需的页面数量。
    ///
    /// ## Returns
    ///
    /// 返回能够包含该地址的最小虚拟页号
    pub fn ceil(&self) -> VirtPageNum {
        if self.0 == 0 {
            VirtPageNum(0)
        } else {
            VirtPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE)
        }
    }

    /// 获取页内偏移
    ///
    /// 返回虚拟地址在页面内的偏移量。该偏移量在地址转换过程中
    /// 直接传递到物理地址，不经过页表转换。
    ///
    /// ## Returns
    ///
    /// 页内偏移量 (0-4095)
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    /// 检查地址是否页对齐
    ///
    /// 判断虚拟地址是否正好位于页面边界上。页对齐的虚拟地址
    /// 适用于页面映射和内存区域的边界检查。
    ///
    /// ## Returns
    ///
    /// 如果地址页对齐返回 `true`，否则返回 `false`
    pub fn aligned(&self) -> bool {
        self.page_offset() == 0
    }
}

impl PhysPageNum {
    /// 获取页表项数组
    ///
    /// 将该物理页面视为页表，返回包含 512 个页表项的可变切片。
    /// 用于直接访问和修改页表内容。
    ///
    /// ## Returns
    ///
    /// 指向 512 个 [`PageTableEntry`] 的可变切片
    ///
    /// ## Safety
    ///
    /// 调用者必须确保：
    /// - 该物理页面确实包含有效的页表数据
    /// - 没有其他代码同时访问同一页表
    /// - 页表项的修改不会破坏内存安全
    ///
    /// ## Examples
    ///
    /// ```
    /// let ppn = PhysPageNum(0x80201);
    /// let ptes = ppn.get_pte_array();
    /// // 访问第一个页表项
    /// let pte = &mut ptes[0];
    /// ```
    pub fn pte_array(&self) -> &'static mut [PageTableEntry] {
        let pa: PhysAddr = self.clone().into();
        unsafe {
            core::slice::from_raw_parts_mut(
                pa.0 as *mut PageTableEntry,
                PAGE_SIZE / core::mem::size_of::<PageTableEntry>(),
            )
        }
    }

    /// 获取页面字节数组
    ///
    /// 将该物理页面作为字节数组访问，返回包含 4096 字节的可变切片。
    /// 用于直接读写页面的原始数据。
    ///
    /// ## Returns
    ///
    /// 指向 4096 字节的可变切片
    ///
    /// ## Safety
    ///
    /// 调用者必须确保：
    /// - 该物理页面已被正确分配
    /// - 没有其他代码同时访问同一页面
    ///
    /// ## Examples
    ///
    /// ```
    /// let ppn = PhysPageNum(0x80201);
    /// let bytes = ppn.get_bytes_array();
    /// bytes[0] = 0x42; // 写入数据
    /// ```
    pub fn bytes_array(&self) -> &'static mut [u8] {
        let pa: PhysAddr = self.clone().into();
        unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, PAGE_SIZE) }
    }

    /// 获取指定类型的可变引用
    ///
    /// 将页面起始地址转换为指定类型的可变引用，用于访问
    /// 存储在页面中的特定数据结构。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 要访问的数据类型
    ///
    /// ## Returns
    ///
    /// 指向类型 `T` 的可变引用
    ///
    /// ## Safety
    ///
    /// 调用者必须确保：
    /// - 页面包含有效的类型 `T` 数据
    /// - 数据已正确初始化
    /// - 类型 `T` 的对齐要求得到满足
    /// - 没有其他代码同时访问同一数据
    ///
    /// ## Examples
    ///
    /// ```
    /// let ppn = PhysPageNum(0x80201);
    /// let data: &mut u64 = ppn.get_mut::<u64>();
    /// *data = 0x1234_5678_9abc_def0;
    /// ```
    pub fn mut_ref<T>(&self) -> &'static mut T {
        let pa: PhysAddr = self.clone().into();
        unsafe { (pa.0 as *mut T).as_mut().unwrap() }
    }
}

impl VirtPageNum {
    /// 获取三级页表索引
    ///
    /// 将 27 位虚拟页号分解为三个 9 位的页表索引，用于 SV39 分页机制
    /// 的三级页表查找。索引从高位到低位分别对应一级、二级、三级页表。
    ///
    /// ## Returns
    ///
    /// 包含三个页表索引的数组 `[VPN[2], VPN[1], VPN[0]]`：
    /// - `[0]`：一级页表索引 (高 9 位)
    /// - `[1]`：二级页表索引 (中 9 位)  
    /// - `[2]`：三级页表索引 (低 9 位)
    ///
    /// 每个索引的范围都是 [0, 511]。
    ///
    /// ## Examples
    ///
    /// ```
    /// let vpn = VirtPageNum(0x12345); // 二进制: 001_001_000_110_100_010_101
    /// let indices = vpn.indexes();
    /// // indices = [0x48, 0x322, 0x45]
    /// // 对应页表索引 [72, 802, 69]
    /// ```
    ///
    /// ## 页表遍历示例
    ///
    /// ```
    /// let indices = vpn.indexes();
    /// let l1_pte = l1_table[indices[0]]; // 一级页表查找
    /// let l2_table = get_next_table(l1_pte);
    /// let l2_pte = l2_table[indices[1]]; // 二级页表查找  
    /// let l3_table = get_next_table(l2_pte);
    /// let l3_pte = l3_table[indices[2]]; // 三级页表查找
    /// ```
    pub fn indexes(&self) -> [usize; 3] {
        let mut vpn = self.0;
        let mut idx = [0usize; 3];
        for i in (0..3).rev() {
            idx[i] = vpn & 511;
            vpn >>= 9;
        }
        idx
    }
}

impl Debug for PhysAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}

impl Debug for VirtAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}

impl Debug for PhysPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#x}", self.0))
    }
}

impl Debug for VirtPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#x}", self.0))
    }
}

impl From<usize> for PhysAddr {
    fn from(value: usize) -> Self {
        Self(value & ((1 << PA_WIDTH_SV39) - 1))
    }
}

impl From<usize> for VirtAddr {
    fn from(value: usize) -> Self {
        Self(value & ((1 << VA_WIDTH_SV39) - 1))
    }
}

impl From<usize> for PhysPageNum {
    fn from(value: usize) -> Self {
        Self(value & ((1 << PPN_WIDTH_SV39) - 1))
    }
}

impl From<usize> for VirtPageNum {
    fn from(value: usize) -> Self {
        Self(value & ((1 << VPN_WIDTH_SV39) - 1))
    }
}

impl From<PhysAddr> for usize {
    fn from(value: PhysAddr) -> Self {
        value.0
    }
}

impl From<PhysPageNum> for usize {
    fn from(value: PhysPageNum) -> Self {
        value.0
    }
}

impl From<VirtAddr> for usize {
    fn from(value: VirtAddr) -> Self {
        if value.0 >= (1 << (VA_WIDTH_SV39 - 1)) {
            value.0 | (!((1 << VA_WIDTH_SV39) - 1))
        } else {
            value.0
        }
    }
}

impl From<VirtPageNum> for usize {
    fn from(value: VirtPageNum) -> Self {
        value.0
    }
}

impl From<PhysAddr> for PhysPageNum {
    fn from(value: PhysAddr) -> Self {
        assert_eq!(value.page_offset(), 0);
        value.floor()
    }
}

impl From<PhysPageNum> for PhysAddr {
    fn from(value: PhysPageNum) -> Self {
        Self(value.0 << PAGE_SIZE_BITS)
    }
}

impl From<VirtAddr> for VirtPageNum {
    fn from(value: VirtAddr) -> Self {
        assert_eq!(value.page_offset(), 0);
        value.floor()
    }
}

impl From<VirtPageNum> for VirtAddr {
    fn from(value: VirtPageNum) -> Self {
        Self(value.0 << PAGE_SIZE_BITS)
    }
}

/// 支持单步递增的类型 trait
///
/// 为支持范围迭代的类型提供递增操作。主要用于页号类型，
/// 使其能够在 [`SimpleRange`] 中进行迭代。
pub trait StepByOne {
    /// 将值递增一步
    ///
    /// 对于页号类型，通常是递增到下一个页号。
    fn step(&mut self);
}

impl StepByOne for VirtPageNum {
    /// 虚拟页号递增
    ///
    /// 将页号加 1，移动到下一个虚拟页面。
    fn step(&mut self) {
        self.0 += 1;
    }
}

/// 简单范围类型
///
/// 提供半开区间 `[start, end)` 的表示和迭代功能，主要用于
/// 虚拟页号范围的管理和批量操作。
///
/// ## 范围语义
///
/// - `start`：起始值（包含）
/// - `end`：结束值（不包含）  
/// - 空范围：`start == end`
///
/// ## 类型约束
///
/// 泛型类型 `T` 必须满足：
/// - [`StepByOne`]：支持单步递增
/// - [`Copy`]：可复制
/// - [`PartialEq`] + [`PartialOrd`]：可比较
/// - [`Debug`]：可调试输出
///
/// ## Examples
///
/// ```
/// let range = SimpleRange::new(
///     VirtPageNum(0x1000),
///     VirtPageNum(0x2000)
/// );
///
/// // 迭代范围内的所有页号
/// for vpn in range {
///     println!("处理页号: {:?}", vpn);
/// }
/// ```
#[derive(Copy, Clone)]
pub struct SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    l: T,
    r: T,
}

impl<T> SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    /// 创建新的范围
    ///
    /// 创建一个从 `start` 到 `end`（不包含）的半开区间范围。
    ///
    /// ## Arguments
    ///
    /// * `start` - 范围起始值（包含）
    /// * `end` - 范围结束值（不包含）
    ///
    /// ## Returns
    ///
    /// 新创建的范围对象
    ///
    /// ## Panics
    ///
    /// 如果 `start > end` 则触发 panic
    ///
    /// ## Examples
    ///
    /// ```
    /// let range = SimpleRange::new(VirtPageNum(100), VirtPageNum(200));
    /// // 创建包含页号 100-199 的范围
    /// ```
    pub fn new(start: T, end: T) -> Self {
        assert!(start <= end, "start {:?} > end {:?}!", start, end);
        Self { l: start, r: end }
    }

    /// 获取范围起始值
    ///
    /// ## Returns
    ///
    /// 范围的起始值（包含在范围内）
    pub fn start(&self) -> T {
        self.l
    }

    /// 获取范围结束值  
    ///
    /// ## Returns
    ///
    /// 范围的结束值（不包含在范围内）
    pub fn end(&self) -> T {
        self.r
    }
}

impl<T> IntoIterator for SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    type Item = T;
    type IntoIter = SimpleRangeIterator<T>;

    /// 将范围转换为迭代器
    ///
    /// 创建一个可以遍历范围内所有值的迭代器，支持 `for` 循环语法。
    ///
    /// ## Returns
    ///
    /// 范围迭代器，按递增顺序产生范围内的每个值
    fn into_iter(self) -> Self::IntoIter {
        SimpleRangeIterator::new(self.l, self.r)
    }
}

/// 简单范围迭代器
///
/// 实现对 [`SimpleRange`] 的迭代访问，按递增顺序遍历范围内的所有值。
/// 迭代器维护当前位置，每次调用 [`next()`] 返回下一个值。
///
/// ## 迭代行为
///
/// - 从起始值开始迭代
/// - 每次递增一步（通过 [`StepByOne::step()`]）
/// - 达到结束值时停止（结束值不包含在内）
/// - 空范围立即结束迭代
pub struct SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    current: T,
    end: T,
}

impl<T> SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    /// 创建新的范围迭代器
    ///
    /// ## Arguments
    ///
    /// * `l` - 迭代起始值
    /// * `r` - 迭代结束值（不包含）
    ///
    /// ## Returns
    ///
    /// 新创建的范围迭代器
    pub fn new(l: T, r: T) -> Self {
        Self { current: l, end: r }
    }
}

impl<T> Iterator for SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    type Item = T;

    /// 返回迭代器的下一个值
    ///
    /// 如果当前位置未达到结束值，返回当前值并递增位置；
    /// 否则返回 `None` 表示迭代结束。
    ///
    /// ## Returns
    ///
    /// - `Some(value)` - 范围内的下一个值
    /// - `None` - 迭代已结束
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.end {
            None
        } else {
            let t = self.current;
            self.current.step();
            Some(t)
        }
    }
}

/// 虚拟页号范围类型别名
///
/// 专门用于虚拟页号的范围表示，提供便捷的类型名称。
/// 常用于虚拟内存区域的批量页面操作。
///
/// ## Examples
///
/// ```
/// // 创建虚拟页号范围
/// let range: VPNRange = SimpleRange::new(
///     VirtPageNum(0x1000),
///     VirtPageNum(0x2000)
/// );
///
/// // 映射范围内的所有页面
/// for vpn in range {
///     map_page(vpn, allocate_frame());
/// }
/// ```
pub type VPNRange = SimpleRange<VirtPageNum>;
