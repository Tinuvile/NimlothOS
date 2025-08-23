//! # Easy File System 核心模块
//!
//! 实现了简单文件系统（Easy File System）的核心功能，包括文件系统的创建、
//! 打开、inode 和数据块的管理等。该模块是文件系统的核心，负责协调各个
//! 组件的工作。
//!
//! ## 文件系统结构
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
//! - [`EasyFileSystem`] - 文件系统主体，管理所有文件系统操作
//! - 位图管理：inode 位图和数据位图，用于资源分配
//! - 区域管理：inode 区域和数据区域的位置计算
//!
//! ## 主要功能
//!
//! - **文件系统创建**：格式化块设备，创建文件系统结构
//! - **文件系统打开**：从现有块设备加载文件系统
//! - **资源分配**：inode 和数据块的分配与回收
//! - **根目录管理**：提供根目录的访问接口
//!
//! ## 使用示例
//!
//! ```rust
//! use easy_fs::{EasyFileSystem, BlockDevice};
//!
//! // 创建新的文件系统
//! let efs = EasyFileSystem::create(block_device, 1024, 1);
//!
//! // 打开现有文件系统
//! let efs = EasyFileSystem::open(block_device);
//!
//! // 获取根目录
//! let root_inode = EasyFileSystem::root_inode(&efs);
//! ```
//!

use super::{
    Bitmap, BlockDevice, DataBlock, DiskInode, DiskInodeType, SuperBlock, block_cache_sync_all,
    get_block_cache,
};
use crate::{BLOCK_SZ, vfs::Inode};
use alloc::sync::Arc;
use spin::Mutex;

/// Easy File System 主体结构
///
/// 管理整个文件系统的状态和操作，包括位图管理、区域定位、资源分配等。
/// 该结构是文件系统的核心，所有文件系统操作都通过它进行。
///
/// ## 字段说明
///
/// - `block_device` - 底层块设备，提供存储能力
/// - `inode_bitmap` - inode 位图，管理 inode 的分配状态
/// - `data_bitmap` - 数据位图，管理数据块的分配状态
/// - `inode_area_start_block` - inode 区域的起始块号
/// - `data_area_start_block` - 数据区域的起始块号
///
/// ## 生命周期
///
/// 文件系统的生命周期包括：
/// 1. **创建阶段**：格式化块设备，初始化所有数据结构
/// 2. **运行阶段**：处理文件操作，管理资源分配
/// 3. **关闭阶段**：同步缓存，确保数据持久化
pub struct EasyFileSystem {
    pub block_device: Arc<dyn BlockDevice>,
    pub inode_bitmap: Bitmap,
    pub data_bitmap: Bitmap,
    inode_area_start_block: u32,
    data_area_start_block: u32,
}

impl EasyFileSystem {
    /// 创建新的文件系统
    ///
    /// 在指定的块设备上创建并格式化一个新的 Easy File System。
    /// 该操作会清空块设备上的所有数据，并初始化文件系统结构。
    ///
    /// ## Arguments
    /// * `block_device` - 要格式化的块设备
    /// * `total_blocks` - 文件系统总块数
    /// * `inode_bitmap_blocks` - inode 位图占用的块数
    ///
    /// ## 创建过程
    /// 1. 计算各个区域的大小和位置
    /// 2. 初始化位图结构
    /// 3. 清空所有数据块
    /// 4. 创建超级块
    /// 5. 创建根目录 inode
    /// 6. 同步所有缓存到磁盘
    ///
    /// ## Returns
    /// 新创建的文件系统实例，包装在 `Arc<Mutex<>>` 中以支持并发访问
    ///
    /// ## 注意事项
    /// - 此操作会清空块设备上的所有现有数据
    /// - 创建完成后会自动分配根目录 inode（ID 为 0）
    /// - 文件系统创建后立即可用
    pub fn create(
        block_device: Arc<dyn BlockDevice>,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
    ) -> Arc<Mutex<Self>> {
        let inode_bitmap = Bitmap::new(1, inode_bitmap_blocks as usize);
        let inode_num = inode_bitmap.maximum();
        let inode_area_blocks =
            ((inode_num * core::mem::size_of::<DiskInode>() + BLOCK_SZ - 1) / BLOCK_SZ) as u32;
        let inode_total_blocks = inode_bitmap_blocks + inode_area_blocks;
        let data_total_blocks = total_blocks - 1 - inode_total_blocks;
        let data_bitmap_blocks = (data_total_blocks + 4096) / 4097;
        let data_area_blocks = data_total_blocks - data_bitmap_blocks;
        let data_bitmap = Bitmap::new(
            (1 + inode_total_blocks) as usize,
            data_bitmap_blocks as usize,
        );
        let mut efs = Self {
            block_device: Arc::clone(&block_device),
            inode_bitmap,
            data_bitmap,
            inode_area_start_block: 1 + inode_bitmap_blocks,
            data_area_start_block: 1 + inode_total_blocks + data_bitmap_blocks,
        };
        for i in 0..total_blocks {
            get_block_cache(i as usize, Arc::clone(&block_device))
                .lock()
                .modify(0, |data_block: &mut DataBlock| {
                    for byte in data_block.iter_mut() {
                        *byte = 0;
                    }
                });
        }
        get_block_cache(0, Arc::clone(&block_device)).lock().modify(
            0,
            |super_block: &mut SuperBlock| {
                super_block.initialize(
                    total_blocks,
                    inode_bitmap_blocks,
                    inode_area_blocks,
                    data_bitmap_blocks,
                    data_area_blocks,
                );
            },
        );
        assert_eq!(efs.alloc_inode(), 0);
        let (root_inode_block_id, root_inode_offset) = efs.get_disk_inode_pos(0);
        get_block_cache(root_inode_block_id as usize, Arc::clone(&block_device))
            .lock()
            .modify(root_inode_offset, |disk_inode: &mut DiskInode| {
                disk_inode.initialize(DiskInodeType::Directory);
            });
        block_cache_sync_all();
        Arc::new(Mutex::new(efs))
    }

    /// 打开现有文件系统
    ///
    /// 从指定的块设备上加载已存在的 Easy File System。
    /// 该操作会读取超级块信息，并恢复文件系统的状态。
    ///
    /// ## Arguments
    /// * `block_device` - 包含文件系统的块设备
    ///
    /// ## 加载过程
    /// 1. 读取块设备第 0 块的超级块
    /// 2. 验证文件系统魔数
    /// 3. 根据超级块信息重建位图结构
    /// 4. 计算各个区域的位置
    ///
    /// ## Returns
    /// 加载的文件系统实例，包装在 `Arc<Mutex<>>` 中以支持并发访问
    ///
    /// ## Panics
    /// 如果块设备不包含有效的 Easy File System 则 panic
    ///
    /// ## 注意事项
    /// - 块设备必须包含有效的文件系统
    /// - 加载过程不会修改块设备上的数据
    /// - 文件系统加载后立即可用
    pub fn open(block_device: Arc<dyn BlockDevice>) -> Arc<Mutex<Self>> {
        get_block_cache(0, Arc::clone(&block_device))
            .lock()
            .read(0, |super_block: &SuperBlock| {
                assert!(super_block.is_valid(), "Error loading EFS!");
                let inode_total_blocks =
                    super_block.inode_bitmap_blocks + super_block.inode_area_blocks;
                let efs = Self {
                    block_device,
                    inode_bitmap: Bitmap::new(1, super_block.inode_bitmap_blocks as usize),
                    data_bitmap: Bitmap::new(
                        (1 + inode_total_blocks) as usize,
                        super_block.data_bitmap_blocks as usize,
                    ),
                    inode_area_start_block: 1 + super_block.inode_bitmap_blocks,
                    data_area_start_block: 1 + inode_total_blocks + super_block.data_bitmap_blocks,
                };
                Arc::new(Mutex::new(efs))
            })
    }

    /// 获取指定 inode 在磁盘上的位置
    ///
    /// 根据 inode ID 计算对应的磁盘 inode 在块设备上的具体位置。
    /// 返回块号和块内偏移量。
    ///
    /// ## Arguments
    /// * `inode_id` - inode 的 ID
    ///
    /// ## Returns
    /// `(block_id, offset)` 元组，表示 inode 所在的块号和块内偏移量
    ///
    /// ## 计算原理
    /// - 每个块可以存储多个 inode（取决于 `DiskInode` 的大小）
    /// - 块号 = inode 区域起始块号 + inode_id / 每块 inode 数量
    /// - 偏移量 = (inode_id % 每块 inode 数量) × inode 大小
    pub fn get_disk_inode_pos(&self, inode_id: u32) -> (u32, usize) {
        let inode_size = core::mem::size_of::<DiskInode>();
        let inodes_per_block = (BLOCK_SZ / inode_size) as u32;
        let block_id = self.inode_area_start_block + inode_id / inodes_per_block;
        (
            block_id,
            (inode_id % inodes_per_block) as usize * inode_size,
        )
    }

    /// 获取数据块在磁盘上的实际块号
    ///
    /// 将逻辑数据块 ID 转换为在块设备上的实际块号。
    /// 数据块 ID 是相对于数据区域的，需要加上数据区域的起始块号。
    ///
    /// ## Arguments
    /// * `data_block_id` - 逻辑数据块 ID
    ///
    /// ## Returns
    /// 数据块在块设备上的实际块号
    pub fn get_data_block_id(&self, data_block_id: u32) -> u32 {
        self.data_area_start_block + data_block_id
    }

    /// 分配一个新的 inode
    ///
    /// 从 inode 位图中分配一个可用的 inode ID。
    /// 该操作会更新位图并返回新分配的 inode ID。
    ///
    /// ## Returns
    /// 新分配的 inode ID
    ///
    /// ## Panics
    /// 如果没有可用的 inode 则 panic
    ///
    /// ## 注意事项
    /// - 分配的 inode 需要后续初始化才能使用
    /// - inode ID 从 0 开始连续分配
    /// - 根目录的 inode ID 固定为 0
    pub fn alloc_inode(&mut self) -> u32 {
        self.inode_bitmap.alloc(&self.block_device).unwrap() as u32
    }

    /// 分配一个新的数据块
    ///
    /// 从数据位图中分配一个可用的数据块，并返回其在块设备上的实际块号。
    /// 该操作会更新位图并返回新分配的数据块号。
    ///
    /// ## Returns
    /// 新分配的数据块在块设备上的实际块号
    ///
    /// ## Panics
    /// 如果没有可用的数据块则 panic
    ///
    /// ## 注意事项
    /// - 返回的是绝对块号，可以直接用于块设备操作
    /// - 分配的数据块内容未初始化
    /// - 数据块分配后需要手动初始化内容
    pub fn alloc_data(&mut self) -> u32 {
        self.data_bitmap.alloc(&self.block_device).unwrap() as u32 + self.data_area_start_block
    }

    /// 释放指定的 inode
    ///
    /// 将指定的 inode 标记为可用，并清空其内容。
    /// 该操作会更新位图并重置 inode 数据。
    ///
    /// ## Arguments
    /// * `inode_id` - 要释放的 inode ID
    ///
    /// ## 当前状态
    /// 此功能目前未实现，仅作为占位符存在。
    /// TODO: 实现文件删除功能
    ///
    /// ## 计划实现
    /// 1. 更新 inode 位图，标记 inode 为可用
    /// 2. 清空 inode 内容，重置为初始状态
    /// 3. 回收 inode 占用的所有数据块
    /// 4. 更新相关的目录项
    pub fn dealloc_inode(&mut self, inode_id: u32) {
        /* TODO: Implement this -> support file delete
        self.inode_bitmap
            .dealloc(&self.block_device, inode_id as usize);
        get_block_cache(
            self.get_disk_inode_pos(inode_id).0 as usize,
            Arc::clone(&self.block_device),
        )
        .lock()
        .modify(
            self.get_disk_inode_pos(inode_id).1,
            |disk_inode: &mut DiskInode| {
                disk_inode.initialize(DiskInodeType::Free);
            },
        );
        */
        return;
    }

    /// 释放指定的数据块
    ///
    /// 将指定的数据块标记为可用，并清空其内容。
    /// 该操作会更新位图并重置数据块内容。
    ///
    /// ## Arguments
    /// * `block_id` - 要释放的数据块在块设备上的实际块号
    ///
    /// ## 操作过程
    /// 1. 清空数据块的所有字节为 0
    /// 2. 更新数据位图，标记块为可用
    /// 3. 将绝对块号转换为相对块号进行位图操作
    ///
    /// ## 注意事项
    /// - `block_id` 必须是绝对块号（相对于整个块设备）
    /// - 释放操作会清空数据块内容
    /// - 释放后的数据块可以重新分配使用
    pub fn dealloc_data(&mut self, block_id: u32) {
        get_block_cache(block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                data_block.iter_mut().for_each(|p| {
                    *p = 0;
                })
            });
        self.data_bitmap.dealloc(
            &self.block_device,
            (block_id - self.data_area_start_block) as usize,
        )
    }

    /// 获取文件系统的根目录 inode
    ///
    /// 创建并返回根目录的 inode 对象，提供对根目录的访问接口。
    /// 根目录是文件系统的入口点，所有其他文件和目录都从根目录开始访问。
    ///
    /// ## Arguments
    /// * `efs` - 文件系统实例的引用
    ///
    /// ## Returns
    /// 根目录的 inode 对象
    ///
    /// ## 根目录特性
    /// - 根目录的 inode ID 固定为 0
    /// - 根目录在文件系统创建时自动初始化
    /// - 根目录始终存在且可访问
    /// - 根目录支持子文件和子目录的创建
    ///
    /// ## 使用示例
    /// ```rust
    /// let root = EasyFileSystem::root_inode(&efs);
    /// let file = root.create("test.txt").unwrap();
    /// ```
    pub fn root_inode(efs: &Arc<Mutex<Self>>) -> Inode {
        let block_device = Arc::clone(&efs.lock().block_device);
        let (block_id, block_offset) = efs.lock().get_disk_inode_pos(0);
        Inode::new(block_id, block_offset, Arc::clone(efs), block_device)
    }
}
