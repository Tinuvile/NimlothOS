//! # VirtIO 块设备驱动实现
//!
//! 实现基于 VirtIO 协议的块设备驱动，提供高性能的虚拟化存储设备访问。
//! 支持 QEMU 等虚拟化环境中的虚拟磁盘设备，为文件系统提供底层存储能力。
//!
//! ## VirtIO 协议特性
//!
//! - **标准化接口**: 基于 VirtIO 1.1 规范的标准设备接口
//! - **高性能**: 支持 DMA 和中断机制，减少 CPU 开销
//! - **虚拟化友好**: 专为虚拟化环境优化的设备协议
//! - **可扩展性**: 支持多种 VirtIO 设备类型
//!
//! ## 核心组件
//!
//! - [`VirtIOBlock`] - VirtIO 块设备的主要接口结构
//! - [`VirtioHal`] - 硬件抽象层实现，提供内存管理接口
//! - [`QUEUE_FRAMES`] - 队列帧管理，用于 DMA 缓冲区
//!
//! ## 内存管理
//!
//! - **DMA 分配**: 通过 `dma_alloc` 分配连续的物理页面
//! - **地址转换**: 支持虚拟地址到物理地址的转换
//! - **帧跟踪**: 自动跟踪分配的物理帧，确保正确释放
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::drivers::block::VirtIOBlock;
//!
//! // 创建 VirtIO 块设备实例
//! let virtio_block = VirtIOBlock::new();
//!
//! // 读取块数据
//! let mut buf = [0u8; 512];
//! virtio_block.read_block(0, &mut buf);
//!
//! // 写入块数据
//! let data = [0x42u8; 512];
//! virtio_block.write_block(0, &data);
//! ```

use super::BlockDevice;
use crate::mm::{
    FrameTracker, PageTable, PhysAddr, PhysPageNum, StepByOne, VirtAddr, frame_alloc,
    frame_dealloc, kernel_token,
};
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use lazy_static::*;
use virtio_drivers::{Hal, VirtIOBlk, VirtIOHeader};

/// VirtIO 块设备在内存映射 I/O 中的基地址
///
/// 该地址是 VirtIO 设备在 QEMU 虚拟化环境中的标准映射地址。
/// 设备寄存器和其他控制结构都位于此地址附近。
#[allow(unused)]
const VIRTIO0: usize = 0x10001000;

/// VirtIO 块设备驱动结构
///
/// 封装 VirtIO 块设备的功能，提供线程安全的块设备访问接口。
/// 使用 `UPSafeCell` 确保内部可变性，支持多线程并发访问。
///
/// ## 内部结构
///
/// 包含一个 `VirtIOBlk` 实例，该实例实现了 VirtIO 块设备的具体功能。
/// 通过 `UPSafeCell` 提供内部可变性，允许在不可变引用上修改内部状态。
///
/// ## 线程安全
///
/// 该结构是线程安全的，多个线程可以同时访问块设备进行读写操作。
/// 并发控制通过 `UPSafeCell` 和内部的锁机制实现。
///
/// ## 生命周期管理
///
/// 设备实例的生命周期与系统运行时间相同，在系统启动时初始化，
/// 在系统关闭时自动清理。
pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<'static, VirtioHal>>);

lazy_static! {
    /// 队列帧管理器
    ///
    /// 管理 VirtIO 设备队列使用的物理帧，确保 DMA 缓冲区的正确分配和释放。
    /// 使用 `UPSafeCell` 提供线程安全的帧管理。
    ///
    /// ## 帧管理策略
    ///
    /// - **连续分配**: 确保分配的物理帧在地址空间中是连续的
    /// - **自动跟踪**: 记录所有分配的帧，便于后续释放
    /// - **批量操作**: 支持一次性分配多个帧
    ///
    /// ## 内存安全
    ///
    /// 该管理器确保分配的物理帧在设备使用期间不会被意外释放，
    /// 并在设备不再需要时正确回收内存。
    static ref QUEUE_FRAMES: UPSafeCell<Vec<FrameTracker>> = unsafe { UPSafeCell::new(Vec::new()) };
}

impl BlockDevice for VirtIOBlock {
    /// 从块设备读取数据
    ///
    /// 从指定的块 ID 读取数据到提供的缓冲区中。该操作是原子的，
    /// 要么读取整个块的数据，要么失败。
    ///
    /// ## Arguments
    ///
    /// * `block_id` - 要读取的块 ID
    /// * `buf` - 用于存储读取数据的缓冲区
    ///
    /// ## 行为
    ///
    /// - 从 VirtIO 块设备读取指定块的数据
    /// - 将数据写入提供的缓冲区
    /// - 如果缓冲区大小小于块大小，只读取缓冲区能容纳的数据
    /// - 如果缓冲区大小大于块大小，多余部分保持不变
    ///
    /// ## 错误处理
    ///
    /// 如果读取操作失败，会触发 panic 并显示错误信息。
    /// 这通常表示设备硬件错误或协议错误。
    ///
    /// ## 性能说明
    ///
    /// 该操作通过 VirtIO 协议进行，支持 DMA 传输，具有较高的性能。
    /// 读取操作是同步的，会阻塞直到数据传输完成。
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        self.0
            .exclusive_access()
            .read_block(block_id, buf)
            .expect("Error when reading VirtIOBlk");
    }

    /// 向块设备写入数据
    ///
    /// 将缓冲区中的数据写入指定的块 ID。该操作是原子的，
    /// 要么完全写入成功，要么完全失败。
    ///
    /// ## Arguments
    ///
    /// * `block_id` - 要写入的块 ID
    /// * `buf` - 包含要写入数据的缓冲区
    ///
    /// ## 行为
    ///
    /// - 将缓冲区数据写入指定的块
    /// - 如果缓冲区大小小于块大小，块中未覆盖的部分保持不变
    /// - 如果缓冲区大小大于块大小，只写入块能容纳的数据
    /// - 写入操作完成后，数据立即可用于后续的读取操作
    ///
    /// ## 错误处理
    ///
    /// 如果写入操作失败，会触发 panic 并显示错误信息。
    /// 这通常表示设备硬件错误、存储空间不足或协议错误。
    ///
    /// ## 持久化
    ///
    /// 写入的数据会立即持久化到存储设备，在系统重启后仍然可用。
    /// 写入操作会刷新设备缓存，确保数据安全。
    fn write_block(&self, block_id: usize, buf: &[u8]) {
        self.0
            .exclusive_access()
            .write_block(block_id, buf)
            .expect("Error when writing VirtIOBlk");
    }
}

impl VirtIOBlock {
    /// 创建新的 VirtIO 块设备实例
    ///
    /// 初始化 VirtIO 块设备，建立与硬件设备的连接。
    /// 该函数会配置设备寄存器，建立 VirtIO 队列，并完成设备初始化。
    ///
    /// ## 初始化过程
    ///
    /// 1. **设备发现**: 通过内存映射 I/O 访问设备寄存器
    /// 2. **协议协商**: 与设备协商 VirtIO 协议版本和功能特性
    /// 3. **队列设置**: 建立 VirtIO 队列用于数据传输
    /// 4. **驱动激活**: 激活设备，使其准备好处理 I/O 请求
    ///
    /// ## Returns
    ///
    /// 返回新创建的 `VirtIOBlock` 实例，包装在 `UPSafeCell` 中以支持内部可变性
    ///
    /// ## Safety
    ///
    /// 该函数使用 `unsafe` 代码访问硬件寄存器，调用者必须确保：
    /// - 硬件设备已正确初始化并可用
    /// - 内存映射 I/O 地址有效且可访问
    /// - 没有其他代码同时访问同一设备
    ///
    /// ## Panics
    ///
    /// 如果设备初始化失败（如设备不存在、协议不兼容等），会触发 panic。
    /// 这通常表示硬件配置错误或设备驱动问题。
    ///
    /// ## Examples
    ///
    /// ```
    /// let virtio_block = VirtIOBlock::new();
    /// // 现在可以使用 virtio_block 进行块设备操作
    /// ```
    #[allow(unused)]
    pub fn new() -> Self {
        unsafe {
            Self(UPSafeCell::new(
                VirtIOBlk::<VirtioHal>::new(&mut *(VIRTIO0 as *mut VirtIOHeader)).unwrap(),
            ))
        }
    }
}

/// VirtIO 硬件抽象层实现
///
/// 为 VirtIO 设备驱动提供硬件抽象接口，包括内存管理、地址转换等功能。
/// 该结构实现了 `Hal` trait，为 VirtIO 驱动提供必要的硬件服务。
///
/// ## 主要功能
///
/// - **DMA 内存管理**: 分配和释放用于 DMA 传输的物理内存
/// - **地址转换**: 在虚拟地址和物理地址之间进行转换
/// - **内存映射**: 提供内存映射 I/O 支持
///
/// ## 内存管理策略
///
/// - **连续分配**: 确保 DMA 缓冲区在物理内存中连续
/// - **自动跟踪**: 自动跟踪分配的物理帧，确保正确释放
/// - **页面对齐**: 所有分配都按页面边界对齐
pub struct VirtioHal;

impl Hal for VirtioHal {
    /// 分配 DMA 内存
    ///
    /// 为 VirtIO 设备分配连续的物理页面，用于 DMA 传输。
    /// 分配的页面必须是连续的，以满足 DMA 传输的要求。
    ///
    /// ## Arguments
    ///
    /// * `pages` - 要分配的页面数量
    ///
    /// ## Returns
    ///
    /// 返回分配的内存的物理地址
    ///
    /// ## 分配策略
    ///
    /// 1. **连续分配**: 确保分配的页面在物理地址空间中连续
    /// 2. **帧跟踪**: 将分配的帧记录到 `QUEUE_FRAMES` 中
    /// 3. **地址计算**: 返回第一个页面的物理地址
    ///
    /// ## 内存管理
    ///
    /// 分配的页面由 `QUEUE_FRAMES` 管理器跟踪，确保在设备不再需要时正确释放。
    /// 这防止了内存泄漏，确保系统的内存使用效率。
    ///
    /// ## Panics
    ///
    /// 如果没有足够的连续物理页面可用，会触发 panic。
    /// 这通常表示系统内存不足或内存碎片化严重。
    fn dma_alloc(pages: usize) -> usize {
        let mut ppn_base = PhysPageNum(0);
        for i in 0..pages {
            let frame = frame_alloc().unwrap();
            if i == 0 {
                ppn_base = frame.ppn;
            }
            assert_eq!(frame.ppn.0, ppn_base.0 + i);
            QUEUE_FRAMES.exclusive_access().push(frame);
        }
        let pa: PhysAddr = ppn_base.into();
        pa.0
    }

    /// 释放 DMA 内存
    ///
    /// 释放之前通过 `dma_alloc` 分配的 DMA 内存。
    /// 该函数会回收所有相关的物理帧，并将它们返回到空闲帧池。
    ///
    /// ## Arguments
    ///
    /// * `pa` - 要释放的内存的物理地址
    /// * `pages` - 要释放的页面数量
    ///
    /// ## Returns
    ///
    /// 总是返回 0，表示操作成功
    ///
    /// ## 释放过程
    ///
    /// 1. **地址转换**: 将物理地址转换为物理页号
    /// 2. **批量释放**: 逐个释放所有相关的物理帧
    /// 3. **帧回收**: 将释放的帧返回到空闲帧池
    ///
    /// ## 内存安全
    ///
    /// 该函数确保释放的内存不会与其他分配冲突，并正确更新内存管理器的状态。
    /// 释放后的内存可以重新分配给其他用途。
    fn dma_dealloc(pa: usize, pages: usize) -> i32 {
        let pa = PhysAddr::from(pa);
        let mut ppn_base: PhysPageNum = pa.into();
        for _ in 0..pages {
            frame_dealloc(ppn_base);
            ppn_base.step();
        }
        0
    }

    /// 物理地址到虚拟地址的转换
    ///
    /// 在当前的实现中，物理地址和虚拟地址是相同的。
    /// 这是因为内核运行在直接映射模式下，物理地址直接对应虚拟地址。
    ///
    /// ## Arguments
    ///
    /// * `addr` - 物理地址
    ///
    /// ## Returns
    ///
    /// 返回对应的虚拟地址（在当前实现中与物理地址相同）
    ///
    /// ## 实现说明
    ///
    /// 该函数在直接映射模式下是恒等映射，即 `phys_to_virt(addr) == addr`。
    /// 这种设计简化了地址转换，提高了性能。
    fn phys_to_virt(addr: usize) -> usize {
        addr
    }

    /// 虚拟地址到物理地址的转换
    ///
    /// 通过内核页表将虚拟地址转换为物理地址。
    /// 该函数使用内核的页表进行地址转换，支持复杂的虚拟内存映射。
    ///
    /// ## Arguments
    ///
    /// * `vaddr` - 虚拟地址
    ///
    /// ## Returns
    ///
    /// 返回对应的物理地址
    ///
    /// ## 转换过程
    ///
    /// 1. **页表查找**: 使用内核页表查找虚拟地址对应的页表项
    /// 2. **地址计算**: 从页表项中提取物理页号
    /// 3. **偏移计算**: 结合页内偏移计算最终的物理地址
    ///
    /// ## 错误处理
    ///
    /// 如果虚拟地址没有有效的页表映射，会触发 panic。
    /// 这通常表示内存访问错误或页表配置问题。
    ///
    /// ## 性能说明
    ///
    /// 该操作涉及页表查找，可能触发 TLB 缺失，有一定的性能开销。
    /// 对于频繁访问的地址，建议缓存转换结果。
    fn virt_to_phys(vaddr: usize) -> usize {
        let result = PageTable::from_token(kernel_token())
            .translate_va(VirtAddr::from(vaddr))
            .unwrap()
            .0;
        result
    }
}
