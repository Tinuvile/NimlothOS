//! # 文件系统布局模块
//!
//! 定义了简单文件系统（Easy File System）的磁盘布局和数据结构。
//! 包含超级块、磁盘 inode、目录项等核心数据结构的定义和操作。
//!
//! ## 文件系统布局
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        SuperBlock                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │                       Inode Bitmap                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                        Inode Area                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │                        Data Bitmap                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                         Data Area                           │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 核心组件
//!
//! - [`SuperBlock`] - 文件系统超级块，包含文件系统元数据
//! - [`DiskInode`] - 磁盘 inode，管理文件的数据块分配
//! - [`DirEntry`] - 目录项，用于目录文件的组织
//!
//! ## 数据块分配策略
//!
//! 采用三级索引结构：
//! - **直接块**：前 28 个数据块直接存储在 inode 中
//! - **一级间接块**：第 29-1024 个数据块通过一级间接块索引
//! - **二级间接块**：第 1025 个及以后的数据块通过二级间接块索引
//!
//! ## 使用示例
//!
//! ```rust
//! use easy_fs::layout::{SuperBlock, DiskInode, DirEntry};
//!
//! // 初始化超级块
//! let mut sb = SuperBlock::default();
//! sb.initialize(1024, 1, 8, 1, 100);
//!
//! // 创建文件 inode
//! let mut inode = DiskInode::default();
//! inode.initialize(DiskInodeType::File);
//!
//! // 创建目录项
//! let entry = DirEntry::new("test.txt", 1);
//! ```
//!

use crate::{BLOCK_SZ, BlockDevice, block_cache};
use alloc::{sync::Arc, vec::Vec};

/// 文件系统魔数，用于标识 Easy File System
const EFS_MAGIC: u32 = 0x3b800001;

/// 文件名长度限制（不包括结尾的 null 字符）
const NAME_LENGTH_LIMIT: usize = 27;

/// 直接数据块数量
const INODE_DIRECT_COUNT: usize = 28;

/// 直接块边界
const DIRECT_BOUND: usize = INODE_DIRECT_COUNT;

/// 一级间接块能索引的数据块数量
const INODE_INDIRECT1_COUNT: usize = BLOCK_SZ / 4;

/// 一级间接块边界
const INDIRECT1_BOUND: usize = DIRECT_BOUND + INODE_INDIRECT1_COUNT;

/// 二级间接块能索引的数据块数量
const INODE_INDIRECT2_COUNT: usize = INODE_INDIRECT1_COUNT * INODE_INDIRECT1_COUNT;

/// 二级间接块边界
const INDIRECT2_BOUND: usize = INDIRECT1_BOUND + INODE_INDIRECT2_COUNT;

/// 目录项大小（字节）
pub const DIRENT_SZ: usize = 32;

/// 间接块类型，存储块 ID 数组
type IndirectBlock = [u32; BLOCK_SZ / 4];

/// 数据块类型，存储文件数据
pub type DataBlock = [u8; BLOCK_SZ];

/// 文件系统超级块
///
/// 存储文件系统的全局元数据，包括文件系统大小、各个区域的块数等信息。
/// 超级块是文件系统的核心数据结构，用于文件系统的识别和初始化。
///
/// ## 布局结构
///
/// ```text
/// ┌─────────┬─────────────────┬─────────────────┬─────────────────┬─────────────────┐
/// │  Magic  │  Total Blocks   │  Inode Bitmap   │   Inode Area    │   Data Bitmap   │
/// │  (4B)   │      (4B)       │      (4B)       │      (4B)       │       (4B)      │
/// └─────────┴─────────────────┴─────────────────┴─────────────────┴─────────────────┘
/// ```
///
/// ## 字段说明
///
/// - `magic` - 文件系统魔数，用于验证文件系统格式
/// - `total_blocks` - 文件系统总块数
/// - `inode_bitmap_blocks` - inode 位图占用的块数
/// - `inode_area_blocks` - inode 区域占用的块数
/// - `data_bitmap_blocks` - 数据位图占用的块数
/// - `data_area_blocks` - 数据区域占用的块数
#[repr(C)]
pub struct SuperBlock {
    magic: u32,
    pub total_blocks: u32,
    pub inode_bitmap_blocks: u32,
    pub inode_area_blocks: u32,
    pub data_bitmap_blocks: u32,
    pub data_area_blocks: u32,
}

/// 磁盘 inode 结构
///
/// 存储文件或目录的元数据，包括文件大小、数据块分配信息等。
/// 支持三级索引结构，能够管理大文件的数据块分配。
///
/// ## 数据块索引结构
///
/// ```text
/// ┌───────────┬───────────────────┬────────────────┬────---───-─────┐
/// │ Size (4B) │ Direct[28] (112B) │ Indirect1 (4B) │ Indirect2 (4B) │
/// └───────────┴───────────────────┴────────────────┴───────---──────┘
/// ```
///
/// ## 索引策略
///
/// - **直接块**：前 28 个数据块直接存储在 `direct` 数组中
/// - **一级间接块**：通过 `indirect1` 指向的块存储块 ID 数组
/// - **二级间接块**：通过 `indirect2` 指向的块存储一级间接块的块 ID 数组
///
/// ## 最大文件大小
///
/// 理论上支持的最大文件大小：
/// - 直接块：28 × 512B = 14KB
/// - 一级间接块：128 × 512B = 64KB
/// - 二级间接块：128 × 128 × 512B = 8MB
/// - 总计：约 8.1MB
#[repr(C)]
pub struct DiskInode {
    pub size: u32,
    pub direct: [u32; INODE_DIRECT_COUNT],
    pub indirect1: u32,
    pub indirect2: u32,
    type_: DiskInodeType,
}

/// 磁盘 inode 类型
///
/// 标识 inode 对应的文件类型，用于区分普通文件和目录。
#[derive(PartialEq)]
pub enum DiskInodeType {
    /// 普通文件
    File,
    /// 目录文件
    Directory,
}

impl SuperBlock {
    /// 初始化超级块
    ///
    /// 使用指定的参数初始化超级块，设置魔数和各个区域的块数。
    ///
    /// ## Arguments
    /// * `total_blocks` - 文件系统总块数
    /// * `inode_bitmap_blocks` - inode 位图块数
    /// * `inode_area_blocks` - inode 区域块数
    /// * `data_bitmap_blocks` - 数据位图块数
    /// * `data_area_blocks` - 数据区域块数
    ///
    /// ## 验证
    /// 确保 `total_blocks` 等于所有区域块数之和
    pub fn initialize(
        &mut self,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
        inode_area_blocks: u32,
        data_bitmap_blocks: u32,
        data_area_blocks: u32,
    ) {
        *self = Self {
            magic: EFS_MAGIC,
            total_blocks,
            inode_bitmap_blocks,
            inode_area_blocks,
            data_bitmap_blocks,
            data_area_blocks,
        }
    }

    /// 验证超级块有效性
    ///
    /// 检查魔数是否匹配，用于验证文件系统格式是否正确。
    ///
    /// ## Returns
    /// 如果魔数匹配返回 `true`，否则返回 `false`
    pub fn valid(&self) -> bool {
        self.magic == EFS_MAGIC
    }
}

impl DiskInode {
    /// 初始化磁盘 inode
    ///
    /// 将 inode 重置为初始状态，清空所有数据块引用并设置文件类型。
    ///
    /// ## Arguments
    /// * `type_` - 文件类型（文件或目录）
    pub fn initialize(&mut self, type_: DiskInodeType) {
        self.size = 0;
        self.direct.iter_mut().for_each(|v| *v = 0);
        self.indirect1 = 0;
        self.indirect2 = 0;
        self.type_ = type_;
    }

    /// 检查是否为目录
    ///
    /// ## Returns
    /// 如果是目录返回 `true`，否则返回 `false`
    pub fn dir(&self) -> bool {
        self.type_ == DiskInodeType::Directory
    }

    /// 检查是否为普通文件
    ///
    /// ## Returns
    /// 如果是普通文件返回 `true`，否则返回 `false`
    pub fn file(&self) -> bool {
        self.type_ == DiskInodeType::File
    }

    /// 获取指定逻辑块号对应的物理块 ID
    ///
    /// 根据三级索引结构查找逻辑块号对应的物理块 ID。
    /// 支持直接块、一级间接块和二级间接块的查找。
    ///
    /// ## Arguments
    /// * `inner_id` - 逻辑块号（从 0 开始）
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    /// 对应的物理块 ID
    ///
    /// ## 索引策略
    /// - 0-27：直接块，从 `direct` 数组获取
    /// - 28-1023：一级间接块，通过 `indirect1` 查找
    /// - 1024+：二级间接块，通过 `indirect2` 查找
    pub fn block_id(&self, inner_id: u32, block_device: &Arc<dyn BlockDevice>) -> u32 {
        let inner_id = inner_id as usize;
        if inner_id < INODE_DIRECT_COUNT {
            self.direct[inner_id]
        } else if inner_id < INDIRECT1_BOUND {
            block_cache(self.indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect_block: &IndirectBlock| {
                    indirect_block[inner_id - INODE_DIRECT_COUNT]
                })
        } else {
            let last = inner_id - INDIRECT1_BOUND;
            let indirect1 = block_cache(self.indirect2 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect2: &IndirectBlock| {
                    indirect2[last / INODE_INDIRECT1_COUNT]
                });
            block_cache(indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect1: &IndirectBlock| {
                    indirect1[last % INODE_INDIRECT1_COUNT]
                })
        }
    }

    /// 计算文件占用的数据块数量
    ///
    /// 根据文件大小计算需要的数据块数量，向上取整。
    ///
    /// ## Returns
    /// 文件占用的数据块数量
    pub fn data_blocks(&self) -> u32 {
        Self::_data_blocks(self.size)
    }

    /// 根据文件大小计算数据块数量
    ///
    /// ## Arguments
    /// * `size` - 文件大小（字节）
    ///
    /// ## Returns
    /// 需要的数据块数量
    fn _data_blocks(size: u32) -> u32 {
        (size + BLOCK_SZ as u32 - 1) / BLOCK_SZ as u32
    }

    /// 计算文件占用的总块数（包括索引块）
    ///
    /// 计算文件占用的所有块数，包括数据块和索引块。
    ///
    /// ## Arguments
    /// * `size` - 文件大小（字节）
    ///
    /// ## Returns
    /// 文件占用的总块数
    pub fn total_blocks(size: u32) -> u32 {
        let data_blocks = Self::_data_blocks(size) as usize;
        let mut total = data_blocks;
        if data_blocks > INODE_DIRECT_COUNT {
            total += 1;
        }
        if data_blocks > INDIRECT1_BOUND {
            total += 1;
            total +=
                (data_blocks - INDIRECT1_BOUND + INODE_INDIRECT1_COUNT - 1) / INODE_INDIRECT1_COUNT;
        }
        total as u32
    }

    /// 计算扩展文件需要的新块数
    ///
    /// 计算将文件从当前大小扩展到新大小需要的新块数。
    ///
    /// ## Arguments
    /// * `new_size` - 新的文件大小
    ///
    /// ## Returns
    /// 需要的新块数
    ///
    /// ## Panics
    /// 如果 `new_size` 小于当前大小则 panic
    pub fn blocks_num_needed(&self, new_size: u32) -> u32 {
        assert!(new_size >= self.size);
        Self::total_blocks(new_size) - Self::total_blocks(self.size)
    }

    /// 扩展文件大小并分配新块
    ///
    /// 将文件扩展到指定大小，并分配新的数据块。支持三级索引结构的
    /// 块分配，自动处理直接块、一级间接块和二级间接块的分配。
    ///
    /// ## Arguments
    /// * `new_size` - 新的文件大小
    /// * `new_blocks` - 新分配的块 ID 列表
    /// * `block_device` - 块设备引用
    ///
    /// ## 分配策略
    /// 1. 优先使用直接块（前 28 个）
    /// 2. 当直接块用完时，分配一级间接块
    /// 3. 当一级间接块用完时，分配二级间接块
    ///
    /// ## 注意事项
    /// - `new_blocks` 的长度必须等于 `blocks_num_needed(new_size)`
    /// - 扩展操作是原子的，要么完全成功，要么完全失败
    pub fn increase_size(
        &mut self,
        new_size: u32,
        new_blocks: Vec<u32>,
        block_device: &Arc<dyn BlockDevice>,
    ) {
        let mut current_blocks = self.data_blocks();
        self.size = new_size;
        let mut total_blocks = self.data_blocks();
        let mut new_blocks = new_blocks.into_iter();

        while current_blocks < total_blocks.min(INODE_DIRECT_COUNT as u32) {
            self.direct[current_blocks as usize] = new_blocks.next().unwrap();
            current_blocks += 1;
        }

        if total_blocks > INODE_DIRECT_COUNT as u32 {
            if current_blocks == INODE_DIRECT_COUNT as u32 {
                self.indirect1 = new_blocks.next().unwrap();
            }
            current_blocks -= INODE_DIRECT_COUNT as u32;
            total_blocks -= INODE_DIRECT_COUNT as u32;
        } else {
            return;
        }
        block_cache(self.indirect1 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect1: &mut IndirectBlock| {
                while current_blocks < total_blocks.min(INODE_INDIRECT1_COUNT as u32) {
                    indirect1[current_blocks as usize] = new_blocks.next().unwrap();
                    current_blocks += 1;
                }
            });

        if total_blocks > INODE_INDIRECT1_COUNT as u32 {
            if current_blocks == INODE_INDIRECT1_COUNT as u32 {
                self.indirect2 = new_blocks.next().unwrap();
            }
            current_blocks -= INODE_INDIRECT1_COUNT as u32;
            total_blocks -= INODE_INDIRECT1_COUNT as u32;
        } else {
            return;
        }
        let mut a0 = current_blocks as usize / INODE_INDIRECT1_COUNT;
        let mut b0 = current_blocks as usize % INODE_INDIRECT1_COUNT;
        let a1 = total_blocks as usize / INODE_INDIRECT1_COUNT;
        let b1 = total_blocks as usize % INODE_INDIRECT1_COUNT;
        block_cache(self.indirect2 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect2: &mut IndirectBlock| {
                while (a0 < a1) || (a0 == a1 && b0 < b1) {
                    if b0 == 0 {
                        indirect2[a0] = new_blocks.next().unwrap();
                    }
                    block_cache(indirect2[a0] as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            indirect1[b0] = new_blocks.next().unwrap();
                        });
                    b0 += 1;
                    if b0 == INODE_INDIRECT1_COUNT {
                        b0 = 0;
                        a0 += 1;
                    }
                }
            });
    }

    /// 清空文件内容并回收所有数据块
    ///
    /// 将文件大小重置为 0，清空所有数据块引用，并返回所有被释放的块 ID。
    /// 支持三级索引结构的块回收。
    ///
    /// ## Arguments
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    /// 被释放的块 ID 列表
    ///
    /// ## 回收策略
    /// 1. 回收直接块
    /// 2. 回收一级间接块及其指向的数据块
    /// 3. 回收二级间接块及其指向的所有块
    pub fn clear_size(&mut self, block_device: &Arc<dyn BlockDevice>) -> Vec<u32> {
        let mut v = Vec::new();
        let mut data_blocks = self.data_blocks() as usize;
        self.size = 0;
        let mut current_blocks = 0usize;

        while current_blocks < data_blocks.min(INODE_DIRECT_COUNT) {
            v.push(self.direct[current_blocks]);
            self.direct[current_blocks] = 0;
            current_blocks += 1;
        }

        if data_blocks > INODE_DIRECT_COUNT {
            v.push(self.indirect1);
            data_blocks -= INODE_DIRECT_COUNT;
            current_blocks = 0;
        } else {
            return v;
        }
        block_cache(self.indirect1 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect1: &mut IndirectBlock| {
                while current_blocks < data_blocks.min(INODE_INDIRECT1_COUNT) {
                    v.push(indirect1[current_blocks]);
                    current_blocks += 1;
                }
            });
        self.indirect1 = 0;

        if data_blocks > INODE_INDIRECT1_COUNT {
            v.push(self.indirect2);
            data_blocks -= INODE_INDIRECT1_COUNT;
        } else {
            return v;
        }
        assert!(data_blocks <= INODE_INDIRECT2_COUNT);
        let a1 = data_blocks / INODE_INDIRECT1_COUNT;
        let b1 = data_blocks % INODE_INDIRECT1_COUNT;
        block_cache(self.indirect2 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect2: &mut IndirectBlock| {
                for entry in indirect2.iter_mut().take(a1) {
                    v.push(*entry);
                    block_cache(*entry as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            for entry in indirect1.iter() {
                                v.push(*entry);
                            }
                        });
                }
                if b1 > 0 {
                    v.push(indirect2[a1]);
                    block_cache(indirect2[a1] as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            for entry in indirect1.iter().take(b1) {
                                v.push(*entry);
                            }
                        });
                }
            });
        self.indirect2 = 0;
        v
    }

    /// 从指定偏移量读取文件数据
    ///
    /// 从文件的指定偏移量开始读取数据到缓冲区中。支持跨块读取，
    /// 自动处理块边界和数据对齐。
    ///
    /// ## Arguments
    /// * `offset` - 读取起始偏移量
    /// * `buf` - 数据缓冲区
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    /// 实际读取的字节数
    ///
    /// ## 读取策略
    /// 1. 计算起始块号和结束块号
    /// 2. 逐块读取数据
    /// 3. 处理块内偏移和跨块读取
    ///
    /// ## 边界处理
    /// - 如果偏移量超出文件大小，返回 0
    /// - 如果缓冲区大小超出文件剩余部分，只读取到文件末尾
    pub fn read_at(
        &self,
        offset: usize,
        buf: &mut [u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        if start >= end {
            return 0;
        }

        let mut start_block = start / BLOCK_SZ;
        let mut read_size = 0usize;
        loop {
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);

            let block_read_size = end_current_block - start;
            let dst = &mut buf[read_size..read_size + block_read_size];
            block_cache(
                self.block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .read(0, |data_block: &DataBlock| {
                let src = &data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_read_size];
                dst.copy_from_slice(src);
            });
            read_size += block_read_size;
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        read_size
    }

    /// 向指定偏移量写入文件数据
    ///
    /// 将缓冲区中的数据写入文件的指定偏移量位置。支持跨块写入，
    /// 自动处理块边界和数据对齐。
    ///
    /// ## Arguments
    /// * `offset` - 写入起始偏移量
    /// * `buf` - 要写入的数据
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    /// 实际写入的字节数
    ///
    /// ## 写入策略
    /// 1. 计算起始块号和结束块号
    /// 2. 逐块写入数据
    /// 3. 处理块内偏移和跨块写入
    ///
    /// ## 注意事项
    /// - 写入操作会修改文件内容
    /// - 如果写入位置超出当前文件大小，需要先扩展文件
    /// - 写入是原子的，要么完全成功，要么完全失败
    pub fn write_at(
        &mut self,
        offset: usize,
        buf: &[u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        assert!(start <= end);

        let mut start_block = start / BLOCK_SZ;
        let mut write_size = 0usize;

        loop {
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);

            let block_write_size = end_current_block - start;
            block_cache(
                self.block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                let src = &buf[write_size..write_size + block_write_size];
                let dst = &mut data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_write_size];
                dst.copy_from_slice(src);
            });
            write_size += block_write_size;
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        write_size
    }
}

/// 目录项结构
///
/// 用于在目录文件中存储文件和子目录的信息。每个目录项包含文件名
/// 和对应的 inode 号，支持目录的层次结构组织。
///
/// ## 布局结构
///
/// ```text
/// ┌────────────┬─────────────┐
/// │ Name (28B) │ Inode (4B)  │
/// └────-───────┴─────────────┘
/// ```
///
/// ## 字段说明
///
/// - `name` - 文件名，固定长度 28 字节，以 null 结尾
/// - `inode_number` - 对应的 inode 号
///
/// ## 大小限制
///
/// - 文件名最大长度：27 个字符（不包括结尾的 null）
/// - 目录项总大小：32 字节
#[repr(C)]
pub struct DirEntry {
    name: [u8; NAME_LENGTH_LIMIT + 1],
    inode_number: u32,
}

impl DirEntry {
    /// 创建空的目录项
    ///
    /// 创建一个未使用的目录项，文件名为空，inode 号为 0。
    ///
    /// ## Returns
    /// 空的目录项
    pub fn empty() -> Self {
        Self {
            name: [0u8; NAME_LENGTH_LIMIT + 1],
            inode_number: 0,
        }
    }

    /// 创建新的目录项
    ///
    /// 使用指定的文件名和 inode 号创建目录项。
    ///
    /// ## Arguments
    /// * `name` - 文件名
    /// * `inode_number` - 对应的 inode 号
    ///
    /// ## 注意事项
    /// - 文件名长度不能超过 27 个字符
    /// - 如果文件名过长，会被截断
    pub fn new(name: &str, inode_number: u32) -> Self {
        let mut bytes = [0u8; NAME_LENGTH_LIMIT + 1];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Self {
            name: bytes,
            inode_number,
        }
    }

    /// 获取目录项的字节表示
    ///
    /// 将目录项转换为字节切片，用于磁盘存储。
    ///
    /// ## Returns
    /// 目录项的字节表示
    ///
    /// ## Safety
    /// 返回的字节切片直接对应结构体的内存布局
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const _ as usize as *const u8, DIRENT_SZ) }
    }

    /// 获取目录项的可变字节表示
    ///
    /// 将目录项转换为可变字节切片，用于磁盘写入。
    ///
    /// ## Returns
    /// 目录项的可变字节表示
    ///
    /// ## Safety
    /// 返回的字节切片直接对应结构体的内存布局
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut _ as usize as *mut u8, DIRENT_SZ) }
    }

    /// 获取文件名
    ///
    /// 从目录项中提取文件名，自动处理 null 结尾。
    ///
    /// ## Returns
    /// 文件名字符串
    ///
    /// ## 注意事项
    /// - 文件名以 null 字符结尾
    /// - 返回的字符串不包含 null 字符
    pub fn name(&self) -> &str {
        let len = (0usize..).find(|i| self.name[*i] == 0).unwrap();
        core::str::from_utf8(&self.name[..len]).unwrap()
    }

    /// 获取 inode 号
    ///
    /// ## Returns
    /// 对应的 inode 号
    pub fn inode_number(&self) -> u32 {
        self.inode_number
    }
}
