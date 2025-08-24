//! # 块缓存管理模块
//!
//! 提供文件系统的块缓存功能，通过内存缓存减少对块设备的频繁访问。
//! 实现了 LRU（最近最少使用）缓存策略，提高文件系统的 I/O 性能。
//!
//! ## 缓存架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    BlockCacheManager                        │
//! │  ┌─────────────┬─────────────┬─────────────┬─────────────┐  │
//! │  │ BlockCache  │ BlockCache  │ BlockCache  │     ...     │  │
//! │  │   (Block 0) │   (Block 1) │   (Block 2) │             │  │
//! │  └─────────────┴─────────────┴─────────────┴─────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 核心功能
//!
//! - **块缓存**: 将块设备数据缓存在内存中
//! - **LRU 替换**: 当缓存满时，替换最近最少使用的块
//! - **延迟写入**: 支持修改标记，延迟同步到磁盘
//! - **类型安全访问**: 提供类型安全的块数据访问接口
//!
//! ## 使用示例
//!
//! ```rust
//! use easy_fs::block_cache::{block_cache, block_cache_sync_all};
//!
//! // 获取块缓存
//! let block_cache = block_cache(block_id, block_device);
//!
//! // 读取数据
//! let data = block_cache.lock().read(offset, |data: &DataBlock| {
//!     data[0..10].to_vec()
//! });
//!
//! // 修改数据
//! block_cache.lock().modify(offset, |data: &mut DataBlock| {
//!     data[0] = 0x42;
//! });
//!
//! // 同步所有缓存到磁盘
//! block_cache_sync_all();
//! ```

use super::{BLOCK_SZ, BlockDevice};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::ptr::{addr_of, addr_of_mut};
use lazy_static::*;
use spin::Mutex;

/// 块缓存大小，限制内存中同时缓存的块数量
///
/// 当缓存满时，会使用 LRU 策略替换最久未使用的块
const BLOCK_CACHE_SIZE: usize = 16;

/// 块缓存结构
///
/// 表示一个块在内存中的缓存，包含块数据和相关的元数据。
/// 支持延迟写入，只有在块被修改时才会写回磁盘。
///
/// ## 内存布局
///
/// ```text
/// ┌─────────────┬─────────────┬─────────────┬─────────────┐
/// │   Cache     │ Block ID    │ BlockDevice │  Modified   │
/// │  [u8; 512]  │   usize     │     Arc     │    bool     │
/// └─────────────┴─────────────┴─────────────┴─────────────┘
/// ```
///
/// ## 字段说明
///
/// - `cache` - 块数据缓存，大小为 512 字节
/// - `block_id` - 对应的块号
/// - `block_device` - 块设备引用
/// - `modified` - 修改标记，表示块是否被修改过
///
/// ## 生命周期管理
///
/// 当 `BlockCache` 被销毁时，如果 `modified` 为 `true`，
/// 会自动将缓存数据写回磁盘。
pub struct BlockCache {
    cache: [u8; BLOCK_SZ],
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
    modified: bool,
}

impl BlockCache {
    /// 创建新的块缓存
    ///
    /// 从块设备读取指定块的数据到内存缓存中。
    ///
    /// ## Arguments
    ///
    /// * `block_id` - 要缓存的块号
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    ///
    /// 新创建的块缓存
    ///
    /// ## 初始化过程
    ///
    /// 1. 分配 512 字节的缓存空间
    /// 2. 从块设备读取块数据到缓存
    /// 3. 初始化元数据（块号、设备引用、修改标记）
    pub fn new(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Self {
        let mut cache = [0u8; BLOCK_SZ];
        block_device.read_block(block_id, &mut cache);
        Self {
            cache,
            block_id,
            block_device,
            modified: false,
        }
    }

    /// 获取指定偏移量的不可变指针
    ///
    /// 返回缓存中指定偏移量位置的不可变指针，用于类型安全的访问。
    ///
    /// ## Arguments
    ///
    /// * `offset` - 缓存内的偏移量
    ///
    /// ## Returns
    ///
    /// 指向指定偏移量的不可变指针
    ///
    /// ## Safety
    ///
    /// 调用者必须确保偏移量在有效范围内（0 <= offset < BLOCK_SZ）
    fn addr_of_offset(&self, offset: usize) -> *const u8 {
        addr_of!(self.cache.as_ref()[offset])
    }

    /// 获取指定偏移量的可变指针
    ///
    /// 返回缓存中指定偏移量位置的可变指针，用于类型安全的修改。
    ///
    /// ## Arguments
    ///
    /// * `offset` - 缓存内的偏移量
    ///
    /// ## Returns
    ///
    /// 指向指定偏移量的可变指针
    ///
    /// ## Safety
    ///
    /// 调用者必须确保偏移量在有效范围内（0 <= offset < BLOCK_SZ）
    fn addr_of_offset_mut(&mut self, offset: usize) -> *mut u8 {
        addr_of_mut!(self.cache.as_mut()[offset])
    }

    /// 获取指定偏移量的不可变引用
    ///
    /// 将缓存中指定偏移量的数据解释为类型 `T` 的不可变引用。
    /// 提供类型安全的数据访问。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 目标数据类型
    ///
    /// ## Arguments
    ///
    /// * `offset` - 缓存内的偏移量
    ///
    /// ## Returns
    ///
    /// 指向类型 `T` 的不可变引用
    ///
    /// ## Panics
    ///
    /// 如果偏移量加上类型 `T` 的大小超出缓存范围，则触发 panic
    ///
    /// ## Examples
    ///
    /// ```
    /// let data: &u32 = block_cache.ref_at(0);  // 读取 u32 数据
    /// let slice: &[u8; 10] = block_cache.ref_at(100);  // 读取字节数组
    /// ```
    pub fn ref_at<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        let addr = self.addr_of_offset(offset);
        unsafe { &*(addr as *const T) }
    }

    /// 获取指定偏移量的可变引用
    ///
    /// 将缓存中指定偏移量的数据解释为类型 `T` 的可变引用。
    /// 提供类型安全的数据修改，并自动设置修改标记。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 目标数据类型
    ///
    /// ## Arguments
    ///
    /// * `offset` - 缓存内的偏移量
    ///
    /// ## Returns
    ///
    /// 指向类型 `T` 的可变引用
    ///
    /// ## Panics
    ///
    /// 如果偏移量加上类型 `T` 的大小超出缓存范围，则触发 panic
    ///
    /// ## 副作用
    ///
    /// 调用此方法会自动设置 `modified` 标记为 `true`
    ///
    /// ## Examples
    ///
    /// ```
    /// let data: &mut u32 = block_cache.mut_at(0);
    /// *data = 0x12345678;  // 修改数据
    /// ```
    pub fn mut_at<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        self.modified = true;
        let addr = self.addr_of_offset_mut(offset);
        unsafe { &mut *(addr as *mut T) }
    }

    /// 读取缓存数据
    ///
    /// 提供函数式接口读取缓存中指定偏移量的数据。
    /// 通过闭包函数安全地访问数据，避免生命周期问题。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 要读取的数据类型
    /// * `V` - 闭包函数的返回类型
    ///
    /// ## Arguments
    ///
    /// * `offset` - 缓存内的偏移量
    /// * `f` - 处理数据的闭包函数
    ///
    /// ## Returns
    ///
    /// 闭包函数的返回值
    ///
    /// ## Examples
    ///
    /// ```
    /// let value = block_cache.read(0, |data: &u32| {
    ///     *data + 1  // 读取并处理数据
    /// });
    /// ```
    pub fn read<T, V>(&self, offset: usize, f: impl FnOnce(&T) -> V) -> V {
        f(self.ref_at(offset))
    }

    /// 修改缓存数据
    ///
    /// 提供函数式接口修改缓存中指定偏移量的数据。
    /// 通过闭包函数安全地修改数据，自动处理修改标记。
    ///
    /// ## Type Parameters
    ///
    /// * `T` - 要修改的数据类型
    /// * `V` - 闭包函数的返回类型
    ///
    /// ## Arguments
    ///
    /// * `offset` - 缓存内的偏移量
    /// * `f` - 修改数据的闭包函数
    ///
    /// ## Returns
    ///
    /// 闭包函数的返回值
    ///
    /// ## 副作用
    ///
    /// 调用此方法会自动设置 `modified` 标记为 `true`
    ///
    /// ## Examples
    ///
    /// ```
    /// block_cache.modify(0, |data: &mut u32| {
    ///     *data = 0xdeadbeef;  // 修改数据
    /// });
    /// ```
    pub fn modify<T, V>(&mut self, offset: usize, f: impl FnOnce(&mut T) -> V) -> V {
        f(self.mut_at(offset))
    }

    /// 同步缓存到磁盘
    ///
    /// 如果缓存被修改过，将缓存数据写回块设备。
    /// 写回后重置修改标记。
    ///
    /// ## 同步策略
    ///
    /// - 只有在 `modified` 为 `true` 时才执行写回
    /// - 写回成功后重置 `modified` 标记
    /// - 写回操作是原子的
    ///
    /// ## Examples
    ///
    /// ```
    /// block_cache.modify(0, |data: &mut u32| { *data = 42; });
    /// block_cache.sync();  // 立即同步到磁盘
    /// ```
    pub fn sync(&mut self) {
        if self.modified {
            self.modified = false;
            self.block_device.write_block(self.block_id, &self.cache);
        }
    }
}

impl Drop for BlockCache {
    /// 析构函数，自动同步缓存到磁盘
    ///
    /// 当 `BlockCache` 被销毁时，如果缓存被修改过，
    /// 自动将数据写回磁盘，确保数据不丢失。
    fn drop(&mut self) {
        self.sync();
    }
}

/// 块缓存管理器
///
/// 管理多个块缓存，实现 LRU（最近最少使用）缓存策略。
/// 当缓存满时，自动替换最久未使用的块缓存。
///
/// ## 缓存策略
///
/// - **容量限制**: 最多同时缓存 `BLOCK_CACHE_SIZE` 个块
/// - **LRU 替换**: 当缓存满时，替换引用计数为 1 的块
/// - **引用计数**: 使用 `Arc` 实现引用计数，支持多个用户共享缓存
///
/// ## 字段说明
///
/// - `queue` - 块缓存队列，存储 (块号, 缓存引用) 对
pub struct BlockCacheManager {
    queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,
}

impl BlockCacheManager {
    /// 创建新的块缓存管理器
    ///
    /// ## Returns
    ///
    /// 新创建的块缓存管理器
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// 获取指定块的缓存
    ///
    /// 如果块已经在缓存中，返回现有的缓存引用；
    /// 否则创建新的缓存并添加到缓存队列中。
    ///
    /// ## Arguments
    ///
    /// * `block_id` - 要获取缓存的块号
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    ///
    /// 块缓存的引用计数智能指针
    ///
    /// ## 缓存管理策略
    ///
    /// 1. **缓存命中**: 如果块已在缓存中，直接返回引用
    /// 2. **缓存未满**: 如果缓存未满，创建新缓存
    /// 3. **缓存已满**: 如果缓存已满，替换引用计数为 1 的块
    /// 4. **替换失败**: 如果无法找到可替换的块，触发 panic
    ///
    /// ## Examples
    ///
    /// ```
    /// let cache = manager.block_cache(block_id, block_device);
    /// let data = cache.lock().read(0, |data: &DataBlock| data[0]);
    /// ```
    pub fn block_cache(
        &mut self,
        block_id: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        if let Some(pair) = self.queue.iter().find(|pair| pair.0 == block_id) {
            Arc::clone(&pair.1)
        } else {
            if self.queue.len() == BLOCK_CACHE_SIZE {
                if let Some((idx, _)) = self
                    .queue
                    .iter()
                    .enumerate()
                    .find(|(_, pair)| Arc::strong_count(&pair.1) == 1)
                {
                    self.queue.drain(idx..=idx);
                } else {
                    panic!("Run out of BlockCache!");
                }
            }
            let block_cache = Arc::new(Mutex::new(BlockCache::new(
                block_id,
                Arc::clone(&block_device),
            )));
            self.queue.push_back((block_id, Arc::clone(&block_cache)));
            block_cache
        }
    }
}

lazy_static! {
    /// 全局块缓存管理器实例
    ///
    /// 使用 `lazy_static` 实现全局单例模式，确保整个文件系统
    /// 共享同一个缓存管理器实例。
    pub static ref BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> =
        Mutex::new(BlockCacheManager::new());
}

/// 获取指定块的缓存
///
/// 全局函数，通过全局缓存管理器获取指定块的缓存。
/// 这是访问块缓存的主要接口。
///
/// ## Arguments
///
/// * `block_id` - 要获取缓存的块号
/// * `block_device` - 块设备引用
///
/// ## Returns
///
/// 块缓存的引用计数智能指针
///
/// ## Examples
///
/// ```
/// let cache = block_cache(block_id, block_device);
/// cache.lock().modify(0, |data: &mut DataBlock| {
///     data[0] = 0x42;
/// });
/// ```
pub fn block_cache(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Arc<Mutex<BlockCache>> {
    BLOCK_CACHE_MANAGER
        .lock()
        .block_cache(block_id, block_device)
}

/// 同步所有缓存到磁盘
///
/// 遍历所有缓存的块，将修改过的块写回磁盘。
/// 通常在文件系统关闭或重要操作完成后调用。
///
/// ## 同步过程
///
/// 1. 获取全局缓存管理器的锁
/// 2. 遍历所有缓存的块
/// 3. 对每个块调用 `sync()` 方法
/// 4. 释放锁
///
/// ## Examples
///
/// ```
/// // 执行文件操作
/// file.write_at(0, &data);
///
/// // 同步所有缓存到磁盘
/// block_cache_sync_all();
/// ```
pub fn block_cache_sync_all() {
    let manager = BLOCK_CACHE_MANAGER.lock();
    for (_, cache) in manager.queue.iter() {
        cache.lock().sync();
    }
}
