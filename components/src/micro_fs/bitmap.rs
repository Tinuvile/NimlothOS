//! # 位图管理模块
//!
//! 提供文件系统中位图（bitmap）的管理功能，用于跟踪和管理数据块和 inode 的分配状态。
//! 位图使用位数组表示资源的使用情况，每个位对应一个资源单元。
//!
//! ## 位图结构
//!
//! ```text
//! ┌───────────-─────────────────┐
//! │         Bitmap Block        │
//! │          [u64; 64]          │
//! └─────────────────────────────┘
//! ```
//!
//! ## 核心功能
//!
//! - **位分配**: 查找并分配第一个可用的位
//! - **位释放**: 释放指定的位，标记为可用
//! - **容量管理**: 计算位图支持的最大位数
//!
//! ## 使用示例
//!
//! ```rust
//! use micro_fs::bitmap::Bitmap;
//!
//! // 创建位图，从块 1 开始，占用 2 个块
//! let bitmap = Bitmap::new(1, 2);
//!
//! // 分配一个位
//! if let Some(bit_id) = bitmap.alloc(&block_device) {
//!     println!("分配了位: {}", bit_id);
//! }
//!
//! // 释放位
//! bitmap.dealloc(&block_device, bit_id);
//! ```

use super::{BLOCK_SZ, BlockDevice, block_cache};
use alloc::sync::Arc;

/// 位图块类型，每个块包含 64 个 u64 整数
///
/// 每个 u64 整数表示 64 个位的状态，总共可以表示 4096 个位（512 字节 × 8 位）
type BitmapBlock = [u64; 64];

/// 每个块包含的位数
///
/// 一个块大小为 512 字节，每个字节 8 位，所以每个块包含 4096 位
const BLOCK_BITS: usize = BLOCK_SZ * 8;

/// 位图管理器
///
/// 管理文件系统中的位图，用于跟踪数据块或 inode 的分配状态。
/// 位图跨越多个块，支持大容量资源的分配管理。
///
/// ## 位图布局
///
/// ```text
/// ┌─────────────┬─────────────┬─────────────┬──────────────┐
/// │  Block 0    │  Block 1    │  Block 2    │     ...      │
/// │  [u64; 64]  │  [u64; 64]  │  [u64; 64]  │              │
/// └─────────────┴─────────────┴─────────────┴──────────────┘
/// ```
///
/// ## 字段说明
///
/// - `start_block_id` - 位图起始块号
/// - `blocks` - 位图占用的块数
///
/// ## 分配策略
///
/// 采用首次适应算法，从第一个块开始查找第一个可用的位。
/// 每个块内部按 u64 分组查找，提高查找效率。
pub struct Bitmap {
    start_block_id: usize,
    blocks: usize,
}

impl Bitmap {
    /// 创建新的位图管理器
    ///
    /// ## Arguments
    ///
    /// * `start_block_id` - 位图起始块号
    /// * `blocks` - 位图占用的块数
    ///
    /// ## Returns
    ///
    /// 新创建的位图管理器
    ///
    /// ## Examples
    ///
    /// ```
    /// let bitmap = Bitmap::new(1, 2); // 从块 1 开始，占用 2 个块
    /// ```
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
        }
    }

    /// 分配一个可用的位
    ///
    /// 在位图中查找第一个可用的位，并将其标记为已使用。
    /// 采用首次适应算法，从第一个块开始顺序查找。
    ///
    /// ## Arguments
    ///
    /// * `block_device` - 块设备引用
    ///
    /// ## Returns
    ///
    /// - `Some(bit_id)` - 成功分配，返回位的全局 ID
    /// - `None` - 没有可用的位
    ///
    /// ## 分配算法
    ///
    /// 1. 遍历所有位图块
    /// 2. 在每个块中查找第一个非满的 u64
    /// 3. 在该 u64 中查找第一个可用的位
    /// 4. 设置该位为已使用
    ///
    /// ## Examples
    ///
    /// ```
    /// if let Some(bit_id) = bitmap.alloc(&block_device) {
    ///     println!("分配了位: {}", bit_id);
    /// } else {
    ///     println!("没有可用的位");
    /// }
    /// ```
    pub fn alloc(&self, block_device: &Arc<dyn BlockDevice>) -> Option<usize> {
        for block_id in 0..self.blocks {
            let pos = block_cache(
                block_id + self.start_block_id as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                if let Some((bits64_pos, inner_pos)) = bitmap_block
                    .iter()
                    .enumerate()
                    .find(|(_, bits64)| **bits64 != u64::MAX)
                    .map(|(bits64_pos, bits64)| (bits64_pos, bits64.trailing_ones() as usize))
                {
                    bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                    Some(block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos as usize)
                } else {
                    None
                }
            });
            if pos.is_some() {
                return pos;
            }
        }
        None
    }

    /// 释放指定的位
    ///
    /// 将指定的位标记为可用，允许后续重新分配。
    /// 通过位分解算法定位到具体的位图块和位位置。
    ///
    /// ## Arguments
    ///
    /// * `block_device` - 块设备引用
    /// * `bit` - 要释放的位的全局 ID
    ///
    /// ## Panics
    ///
    /// 如果指定的位已经是可用状态，则触发 panic
    ///
    /// ## 位分解算法
    ///
    /// 将全局位 ID 分解为：
    /// - 块内位置：`bit / BLOCK_BITS`
    /// - u64 索引：`(bit % BLOCK_BITS) / 64`
    /// - 位内位置：`(bit % BLOCK_BITS) % 64`
    ///
    /// ## Examples
    ///
    /// ```
    /// bitmap.dealloc(&block_device, 1024); // 释放位 1024
    /// ```
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        block_cache(block_pos + self.start_block_id, Arc::clone(block_device))
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
                bitmap_block[bits64_pos] -= 1u64 << inner_pos;
            });
    }

    /// 获取位图支持的最大位数
    ///
    /// 计算位图能够管理的最大位数，等于所有块包含的位数总和。
    ///
    /// ## Returns
    ///
    /// 位图支持的最大位数
    ///
    /// ## Examples
    ///
    /// ```
    /// let max_bits = bitmap.maximum(); // 例如：8192 (2 块 × 4096 位/块)
    /// ```
    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}

/// 位分解函数
///
/// 将全局位 ID 分解为块内位置、u64 索引和位内位置。
/// 用于在位图中定位具体的位位置。
///
/// ## Arguments
///
/// * `bit` - 全局位 ID
///
/// ## Returns
///
/// 三元组 `(block_pos, bits64_pos, inner_pos)`：
/// - `block_pos` - 块内位置（相对于位图起始块）
/// - `bits64_pos` - u64 数组中的索引
/// - `inner_pos` - u64 内的位位置
///
/// ## 分解公式
///
/// ```rust
/// let block_pos = bit / BLOCK_BITS;           // 块位置
/// let bits64_pos = (bit % BLOCK_BITS) / 64;   // u64 索引
/// let inner_pos = (bit % BLOCK_BITS) % 64;    // 位位置
/// ```
fn decomposition(mut bit: usize) -> (usize, usize, usize) {
    let block_pos = bit / BLOCK_BITS;
    bit = bit % BLOCK_BITS;
    (block_pos, bit / 64, bit % 64)
}
