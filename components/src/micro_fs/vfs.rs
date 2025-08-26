//! # 虚拟文件系统接口模块
//!
//! 提供 Micro File System 的虚拟文件系统（VFS）接口，为用户程序提供统一的文件操作 API。
//! 封装了底层文件系统的复杂性，提供简洁易用的文件管理接口。
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     User Application                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                     VFS Interface                           │
//! │  ┌─────────────┬─────────────┬─────────────┬─────────────┐  │
//! │  │    find     │    create   │   read_at   │  write_at   │  │
//! │  └─────────────┴─────────────┴─────────────┴─────────────┘  │
//! ├─────────────────────────────────────────────────────────────┤
//! │                     Micro File System                        │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 核心功能
//!
//! - **文件查找**: 在目录中查找指定名称的文件或子目录
//! - **文件创建**: 创建新的文件或目录
//! - **文件读写**: 支持随机访问的文件读写操作
//! - **目录管理**: 列出目录内容，管理目录结构
//! - **文件清理**: 清空文件内容，回收存储空间
//!
//! ## 使用示例
//!
//! ```rust
//! use micro_fs::vfs::Inode;
//!
//! // 查找文件
//! if let Some(file) = root_inode.find("test.txt") {
//!     // 读取文件内容
//!     let mut buf = [0u8; 1024];
//!     let bytes_read = file.read_at(0, &mut buf);
//! }
//!
//! // 创建新文件
//! if let Some(new_file) = root_inode.create("new.txt") {
//!     // 写入数据
//!     new_file.write_at(0, b"Hello, World!");
//! }
//!
//! // 列出目录内容
//! let files = root_inode.ls();
//! for file in files {
//!     println!("文件: {}", file);
//! }
//! ```

use super::{
    BlockDevice, DIRENT_SZ, DirEntry, DiskInode, DiskInodeType, MicroFileSystem, block_cache,
    block_cache_sync_all,
};
use alloc::{string::String, sync::Arc, vec::Vec};
use spin::{Mutex, MutexGuard};

/// 文件系统 inode 接口
///
/// 表示文件系统中的一个文件或目录，提供统一的文件操作接口。
/// 封装了底层磁盘 inode 的复杂性，提供类型安全和易用的 API。
///
/// ## 内存布局
///
/// ```text
/// ┌─────────────┬─────────────────┬─────────────────┬─────────────────┐
/// │  Block ID   │   Block Offset  │   File System   │   Block Device  │
/// │   usize     │     usize       │      Arc        │      Arc        │
/// └─────────────┴─────────────────┴─────────────────┴─────────────────┘
/// ```
///
/// ## 字段说明
///
/// - `block_id` - 磁盘 inode 所在的块号
/// - `block_offset` - 磁盘 inode 在块内的偏移量
/// - `fs` - 文件系统实例的引用计数智能指针
/// - `block_device` - 块设备接口的引用计数智能指针
///
/// ## 生命周期管理
///
/// 使用引用计数智能指针管理文件系统和块设备的生命周期，
/// 确保在 inode 存在期间相关资源不会被释放。
///
/// ## 并发安全
///
/// 通过文件系统内部的互斥锁保证并发访问的安全性。
/// 多个线程可以同时访问不同的 inode，但同一 inode 的并发访问会被序列化。
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<MicroFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// 创建新的 inode 接口
    ///
    /// 根据磁盘 inode 的位置信息创建 inode 接口对象。
    ///
    /// ## Arguments
    ///
    /// * `block_id` - 磁盘 inode 所在的块号
    /// * `block_offset` - 磁盘 inode 在块内的偏移量
    /// * `fs` - 文件系统实例的引用计数智能指针
    /// * `block_device` - 块设备接口的引用计数智能指针
    ///
    /// ## Returns
    ///
    /// 新创建的 inode 接口对象
    ///
    /// ## 注意事项
    ///
    /// - 调用者必须确保 `block_id` 和 `block_offset` 指向有效的磁盘 inode
    /// - 文件系统和块设备必须保持有效状态
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<MicroFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }

    /// 读取磁盘 inode 数据
    ///
    /// 从块缓存中读取磁盘 inode 数据，并通过闭包函数进行处理。
    /// 提供类型安全的磁盘 inode 访问接口。
    ///
    /// ## Type Parameters
    ///
    /// * `V` - 闭包函数的返回类型
    ///
    /// ## Arguments
    ///
    /// * `f` - 处理磁盘 inode 数据的闭包函数
    ///
    /// ## Returns
    ///
    /// 闭包函数的返回值
    ///
    /// ## Examples
    ///
    /// ```
    /// let size = self.read_disk_inode(|disk_inode| {
    ///     disk_inode.size
    /// });
    /// ```
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }

    /// 修改磁盘 inode 数据
    ///
    /// 从块缓存中获取磁盘 inode 的可变引用，并通过闭包函数进行修改。
    /// 提供类型安全的磁盘 inode 修改接口。
    ///
    /// ## Type Parameters
    ///
    /// * `V` - 闭包函数的返回类型
    ///
    /// ## Arguments
    ///
    /// * `f` - 修改磁盘 inode 数据的闭包函数
    ///
    /// ## Returns
    ///
    /// 闭包函数的返回值
    ///
    /// ## 副作用
    ///
    /// 调用此方法会修改磁盘 inode 的内容，修改会被缓存并最终写回磁盘
    ///
    /// ## Examples
    ///
    /// ```
    /// self.modify_disk_inode(|disk_inode| {
    ///     disk_inode.size = new_size;
    /// });
    /// ```
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }

    /// 查找指定名称的文件或目录
    ///
    /// 在当前目录中查找指定名称的文件或子目录。
    /// 如果找到，返回对应的 inode 接口对象。
    ///
    /// ## Arguments
    ///
    /// * `name` - 要查找的文件或目录名称
    ///
    /// ## Returns
    ///
    /// - `Some(inode)` - 找到文件或目录，返回对应的 inode
    /// - `None` - 未找到指定名称的文件或目录
    ///
    /// ## 查找过程
    ///
    /// 1. 获取文件系统锁，确保并发安全
    /// 2. 读取当前目录的磁盘 inode
    /// 3. 遍历目录项，查找匹配的文件名
    /// 4. 如果找到，创建并返回对应的 inode 接口
    ///
    /// ## 注意事项
    ///
    /// - 当前 inode 必须是目录类型
    /// - 文件名区分大小写
    /// - 返回的 inode 共享相同的文件系统和块设备引用
    ///
    /// ## Examples
    ///
    /// ```
    /// if let Some(file) = root_inode.find("config.txt") {
    ///     println!("找到文件: config.txt");
    ///     // 使用 file 进行文件操作
    /// }
    /// ```
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }

    /// 在目录中查找指定名称的 inode ID
    ///
    /// 遍历目录项，查找指定名称对应的 inode 号。
    /// 这是 `find` 方法的内部实现。
    ///
    /// ## Arguments
    ///
    /// * `name` - 要查找的文件或目录名称
    /// * `disk_inode` - 当前目录的磁盘 inode
    ///
    /// ## Returns
    ///
    /// - `Some(inode_id)` - 找到对应的 inode 号
    /// - `None` - 未找到指定名称的文件或目录
    ///
    /// ## 查找算法
    ///
    /// 1. 计算目录中的文件数量
    /// 2. 遍历所有目录项
    /// 3. 比较目录项名称与目标名称
    /// 4. 找到匹配项时返回对应的 inode 号
    ///
    /// ## Panics
    ///
    /// 如果当前 inode 不是目录类型，则触发 panic
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        assert!(disk_inode.dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_number() as u32);
            }
        }
        None
    }

    /// 列出目录内容
    ///
    /// 返回当前目录中所有文件和子目录的名称列表。
    ///
    /// ## Returns
    ///
    /// 包含所有文件和目录名称的字符串向量
    ///
    /// ## 列出过程
    ///
    /// 1. 获取文件系统锁，确保并发安全
    /// 2. 读取当前目录的磁盘 inode
    /// 3. 计算目录中的文件数量
    /// 4. 遍历所有目录项，提取文件名
    /// 5. 返回文件名列表
    ///
    /// ## 注意事项
    ///
    /// - 当前 inode 必须是目录类型
    /// - 返回的名称不包含路径信息
    /// - 名称按目录项存储顺序返回
    ///
    /// ## Examples
    ///
    /// ```
    /// let files = root_inode.ls();
    /// for file in files {
    ///     println!("文件: {}", file);
    /// }
    /// ```
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }

    /// 创建新文件
    ///
    /// 在当前目录中创建指定名称的新文件。
    /// 如果文件已存在，返回 `None`。
    ///
    /// ## Arguments
    ///
    /// * `name` - 新文件的名称
    ///
    /// ## Returns
    ///
    /// - `Some(inode)` - 成功创建文件，返回新文件的 inode
    /// - `None` - 文件已存在，创建失败
    ///
    /// ## 创建过程
    ///
    /// 1. 检查文件是否已存在
    /// 2. 分配新的 inode
    /// 3. 初始化新 inode 为文件类型
    /// 4. 在当前目录中添加目录项
    /// 5. 同步缓存到磁盘
    /// 6. 返回新文件的 inode
    ///
    /// ## 注意事项
    ///
    /// - 当前 inode 必须是目录类型
    /// - 文件名不能超过 27 个字符
    /// - 创建操作是原子的，要么完全成功，要么完全失败
    ///
    /// ## Examples
    ///
    /// ```
    /// if let Some(new_file) = root_inode.create("new.txt") {
    ///     println!("成功创建文件: new.txt");
    ///     // 使用 new_file 进行文件操作
    /// } else {
    ///     println!("文件已存在或创建失败");
    /// }
    /// ```
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            assert!(root_inode.dir());
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }

        let new_inode_id = fs.alloc_inode();
        let (new_inode_block_id, new_inode_block_offset) = fs.disk_inode_pos(new_inode_id);
        block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, root_inode, &mut fs);
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
    }

    /// 扩展文件大小
    ///
    /// 将文件扩展到指定大小，并分配必要的数据块。
    /// 这是文件创建和写入操作的内部辅助方法。
    ///
    /// ## Arguments
    ///
    /// * `new_size` - 新的文件大小
    /// * `disk_inode` - 要扩展的磁盘 inode
    /// * `fs` - 文件系统的可变引用
    ///
    /// ## 扩展过程
    ///
    /// 1. 检查新大小是否大于当前大小
    /// 2. 计算需要的新块数
    /// 3. 分配新的数据块
    /// 4. 更新磁盘 inode 的块分配信息
    ///
    /// ## 注意事项
    ///
    /// - 只有在需要扩展时才执行分配操作
    /// - 分配操作通过文件系统管理器进行
    /// - 扩展操作是原子的
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<MicroFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }

    /// 清空文件内容
    ///
    /// 将文件大小重置为 0，清空所有数据块，并回收存储空间。
    /// 文件本身不会被删除，只是内容被清空。
    ///
    /// ## 清空过程
    ///
    /// 1. 获取文件系统锁
    /// 2. 读取当前文件大小
    /// 3. 清空磁盘 inode 的块分配信息
    /// 4. 回收所有数据块（当前实现中未实际回收）
    /// 5. 同步缓存到磁盘
    ///
    /// ## 注意事项
    ///
    /// - 清空操作是原子的
    /// - 文件类型和权限保持不变
    /// - 当前实现中数据块回收功能未完全实现
    ///
    /// ## Examples
    ///
    /// ```
    /// file.clear(); // 清空文件内容
    /// ```
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }

    /// 从指定偏移量读取文件数据
    ///
    /// 从文件的指定偏移量开始读取数据到缓冲区中。
    /// 支持跨块读取，自动处理块边界。
    ///
    /// ## Arguments
    ///
    /// * `offset` - 读取起始偏移量
    /// * `buf` - 数据缓冲区
    ///
    /// ## Returns
    ///
    /// 实际读取的字节数
    ///
    /// ## 读取策略
    ///
    /// 1. 获取文件系统锁，确保并发安全
    /// 2. 读取磁盘 inode 获取文件大小
    /// 3. 计算起始块号和结束块号
    /// 4. 逐块读取数据到缓冲区
    /// 5. 处理块内偏移和跨块读取
    ///
    /// ## 边界处理
    ///
    /// - 如果偏移量超出文件大小，返回 0
    /// - 如果缓冲区大小超出文件剩余部分，只读取到文件末尾
    /// - 支持读取 0 字节（返回 0）
    ///
    /// ## Examples
    ///
    /// ```
    /// let mut buf = [0u8; 1024];
    /// let bytes_read = file.read_at(0, &mut buf);
    /// println!("读取了 {} 字节", bytes_read);
    /// ```
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }

    /// 向指定偏移量写入文件数据
    ///
    /// 将缓冲区中的数据写入文件的指定偏移量位置。
    /// 支持跨块写入，自动扩展文件大小。
    ///
    /// ## Arguments
    ///
    /// * `offset` - 写入起始偏移量
    /// * `buf` - 要写入的数据
    ///
    /// ## Returns
    ///
    /// 实际写入的字节数
    ///
    /// ## 写入策略
    ///
    /// 1. 获取文件系统锁
    /// 2. 计算新的文件大小
    /// 3. 如果需要，扩展文件大小并分配新块
    /// 4. 逐块写入数据
    /// 5. 同步缓存到磁盘
    ///
    /// ## 扩展行为
    ///
    /// - 如果写入位置超出当前文件大小，自动扩展文件
    /// - 扩展操作会分配必要的数据块
    /// - 写入操作是原子的
    ///
    /// ## 注意事项
    ///
    /// - 写入操作会修改文件内容
    /// - 支持写入 0 字节（返回 0）
    /// - 写入完成后会自动同步到磁盘
    ///
    /// ## Examples
    ///
    /// ```
    /// let data = b"Hello, World!";
    /// let bytes_written = file.write_at(0, data);
    /// println!("写入了 {} 字节", bytes_written);
    /// ```
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
}
