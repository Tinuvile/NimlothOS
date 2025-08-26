//! # 块设备驱动模块
//!
//! 提供块设备的抽象接口和具体实现，支持磁盘、存储设备等块级 I/O 操作。
//! 通过统一的块设备接口，为文件系统提供底层存储访问能力。
//!
//! ## 模块组织
//!
//! - [`virtio_blk`] - VirtIO 块设备驱动实现
//! - 全局块设备实例 [`BLOCK_DEVICE`]
//!
//! ## 块设备特性
//!
//! - **固定块大小**: 所有块设备使用相同的块大小（通常为 512 字节）
//! - **随机访问**: 支持任意块 ID 的读写操作
//! - **原子操作**: 单个块的读写操作是原子的
//! - **并发安全**: 支持多线程并发访问
//!
//! ## 核心组件
//!
//! ### 块设备接口
//! - [`BlockDevice`] - 块设备抽象 trait，定义基本的块读写操作
//! - [`VirtIOBlock`] - VirtIO 块设备的具体实现
//!
//! ### 全局实例
//! - [`BLOCK_DEVICE`] - 全局块设备实例，提供统一的块设备访问接口
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::drivers::block::BLOCK_DEVICE;
//!
//! // 读取块设备数据
//! let mut buf = [0u8; 512];
//! BLOCK_DEVICE.read_block(0, &mut buf);
//!
//! // 写入块设备数据
//! let data = [0x42u8; 512];
//! BLOCK_DEVICE.write_block(0, &data);
//!
//! // 创建自定义块设备
//! let virtio_block = VirtIOBlock::new();
//! virtio_block.read_block(1, &mut buf);
//! ```
//!
//! ## 性能特点
//!
//! - **缓存优化**: 支持块级缓存，减少实际 I/O 操作
//! - **批量操作**: 支持批量读写，提高 I/O 效率
//! - **异步支持**: 支持异步 I/O 操作（待实现）
//! - **DMA 支持**: 支持直接内存访问，减少 CPU 开销

use crate::board::BlockDeviceImpl;
use alloc::sync::Arc;
use components::easy_fs::BlockDevice;
use lazy_static::*;

mod virtio_blk;

pub use virtio_blk::VirtIOBlock;

lazy_static! {
    /// 全局块设备实例
    ///
    /// 使用 `lazy_static` 实现全局单例模式，确保整个系统共享同一个块设备实例。
    /// 该实例在首次访问时初始化，提供对底层存储设备的统一访问接口。
    ///
    /// ## 初始化过程
    ///
    /// 1. 创建 `BlockDeviceImpl` 实例（通常是 VirtIO 块设备）
    /// 2. 包装在 `Arc` 中以支持多线程共享访问
    /// 3. 注册为全局块设备实例
    ///
    /// ## 使用场景
    ///
    /// - 文件系统的底层存储访问
    /// - 系统镜像的读写操作
    /// - 用户数据的持久化存储
    ///
    /// ## 线程安全
    ///
    /// 该实例是线程安全的，多个线程可以同时访问块设备进行读写操作。
    /// 具体的并发控制由底层设备驱动实现。
    ///
    /// ## Examples
    ///
    /// ```
    /// use crate::drivers::block::BLOCK_DEVICE;
    ///
    /// // 读取第一个块
    /// let mut buf = [0u8; 512];
    /// BLOCK_DEVICE.read_block(0, &mut buf);
    ///
    /// // 写入数据到第二个块
    /// let data = [0x42u8; 512];
    /// BLOCK_DEVICE.write_block(1, &data);
    /// ```
    pub static ref BLOCK_DEVICE: Arc<dyn BlockDevice> = {
        let block_device = Arc::new(BlockDeviceImpl::new());
        block_device
    };
}
