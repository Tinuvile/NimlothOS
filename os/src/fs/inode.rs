//! # 文件 Inode 管理模块
//!
//! 提供操作系统级别的文件 inode 管理功能，包括文件读写、元数据操作、
//! 文件系统访问等功能。封装底层 Micro-FS 文件系统，为上层提供统一的文件接口。
//!
//! ## 核心组件
//!
//! - [`OSInode`] - 操作系统级别的 inode 封装，提供文件操作接口
//! - [`OSInodeInner`] - inode 的内部状态管理
//! - [`OpenFlags`] - 文件打开标志位，控制文件的打开模式
//! - [`ROOT_INODE`] - 全局根目录 inode 实例
//!
//! ## 文件操作特性
//!
//! - **读写权限**: 支持独立的读写权限控制
//! - **位置管理**: 自动管理文件偏移量
//! - **批量操作**: 支持一次性读取整个文件内容
//! - **文件创建**: 支持新文件的创建和现有文件的截断
//!
//! ## 使用示例
//!
//! ```rust
//! use crate::fs::{open_file, OpenFlags};
//!
//! // 打开文件进行读写
//! let file = open_file("test.txt", OpenFlags::RDWR).unwrap();
//!
//! // 读取整个文件内容
//! let content = file.read_all();
//!
//! // 列出应用程序
//! list_apps();
//! ```

use super::File;
use crate::drivers::BLOCK_DEVICE;
use crate::mm::UserBuffer;
use crate::println;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::*;
use components::micro_fs::{Inode, MicroFileSystem};
use lazy_static::*;

/// OSInode 的内部状态结构
///
/// 管理文件 inode 的内部状态，包括文件偏移量和底层 inode 引用。
/// 该结构被封装在 `UPSafeCell` 中，提供线程安全的内部可变性。
///
/// ## 字段说明
///
/// - `offset` - 当前文件偏移量，表示下次读写操作的位置
/// - `inode` - 底层 Micro-FS inode 的引用计数智能指针
///
/// ## 状态管理
///
/// - 文件偏移量在每次读写操作后自动更新
/// - inode 引用确保底层文件系统资源不会被过早释放
/// - 通过 `UPSafeCell` 提供线程安全的内部状态修改
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

/// 操作系统级别的文件 Inode
///
/// 封装底层 Micro-FS 文件系统的 inode，提供操作系统级别的文件操作接口。
/// 支持读写权限控制、文件偏移量管理和批量文件操作。
///
/// ## 内部结构
///
/// 包含读写权限标志和内部状态管理结构，通过 `UPSafeCell` 提供线程安全。
///
/// ## 线程安全
///
/// 该结构是线程安全的，多个线程可以同时访问同一个文件进行读写操作。
/// 并发控制通过 `UPSafeCell` 和内部的锁机制实现。
///
/// ## 生命周期管理
///
/// 文件实例的生命周期由引用计数管理，当所有引用都被释放时，
/// 底层文件系统资源会被自动清理。
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}

impl OSInode {
    /// 创建新的 OSInode 实例
    ///
    /// 根据指定的读写权限和底层 inode 创建新的文件实例。
    /// 新创建的文件偏移量初始化为 0。
    ///
    /// ## Arguments
    ///
    /// * `readable` - 文件是否可读
    /// * `writable` - 文件是否可写
    /// * `inode` - 底层 Micro-FS inode 的引用
    ///
    /// ## Returns
    ///
    /// 新创建的 `OSInode` 实例
    ///
    /// ## 初始化状态
    ///
    /// - 文件偏移量设置为 0
    /// - 读写权限根据参数设置
    /// - 内部状态通过 `UPSafeCell` 保护
    ///
    /// ## Examples
    ///
    /// ```
    /// let inode = get_some_inode();
    /// let file = OSInode::new(true, false, inode); // 只读文件
    /// ```
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
        }
    }

    /// 读取整个文件内容
    ///
    /// 从文件开头开始读取所有内容，直到文件末尾。
    /// 该操作会重置文件偏移量到开头，然后连续读取直到文件结束。
    ///
    /// ## Returns
    ///
    /// 包含整个文件内容的字节向量
    ///
    /// ## 读取策略
    ///
    /// 1. **重置偏移量**: 将文件偏移量设置为 0
    /// 2. **批量读取**: 使用 512 字节的缓冲区进行批量读取
    /// 3. **连续读取**: 循环读取直到到达文件末尾
    /// 4. **内容收集**: 将所有读取的内容合并到一个向量中
    ///
    /// ## 性能说明
    ///
    /// 该操作会读取整个文件，对于大文件可能消耗较多内存和时间。
    /// 建议只对小文件使用此方法，大文件应该使用流式读取。
    ///
    /// ## 内存使用
    ///
    /// 返回的向量大小等于文件大小，会占用相应的内存空间。
    /// 调用者负责管理返回的内存。
    ///
    /// ## Examples
    ///
    /// ```
    /// let content = file.read_all();
    /// println!("文件大小: {} 字节", content.len());
    /// ```
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buffer = [0u8; 512];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

impl File for OSInode {
    /// 从文件读取数据到用户缓冲区
    ///
    /// 从当前文件偏移量开始读取数据，支持跨页面的用户缓冲区。
    /// 该操作会更新文件偏移量，为下次读取做准备。
    ///
    /// ## Arguments
    ///
    /// * `buf` - 用户缓冲区，可能跨越多个页面
    ///
    /// ## Returns
    ///
    /// 实际读取的字节数，0 表示已到达文件末尾
    ///
    /// ## 读取过程
    ///
    /// 1. **权限检查**: 验证文件是否可读
    /// 2. **缓冲区遍历**: 遍历用户缓冲区的所有页面
    /// 3. **数据读取**: 从当前偏移量读取数据到每个页面
    /// 4. **偏移量更新**: 更新文件偏移量
    /// 5. **提前结束**: 如果到达文件末尾则停止读取
    ///
    /// ## 错误处理
    ///
    /// 如果文件不可读，行为由具体实现定义。
    /// 读取过程中如果遇到错误，会返回已读取的字节数。
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }

    /// 向文件写入数据
    ///
    /// 从当前文件偏移量开始写入数据，支持跨页面的用户缓冲区。
    /// 该操作会更新文件偏移量，为下次写入做准备。
    ///
    /// ## Arguments
    ///
    /// * `buf` - 用户缓冲区，包含要写入的数据
    ///
    /// ## Returns
    ///
    /// 实际写入的字节数
    ///
    /// ## 写入过程
    ///
    /// 1. **权限检查**: 验证文件是否可写
    /// 2. **缓冲区遍历**: 遍历用户缓冲区的所有页面
    /// 3. **数据写入**: 将每个页面的数据写入文件
    /// 4. **偏移量更新**: 更新文件偏移量
    /// 5. **完整性检查**: 确保写入的字节数与缓冲区大小一致
    ///
    /// ## 错误处理
    ///
    /// 如果文件不可写，会触发 panic。
    /// 如果写入的字节数与缓冲区大小不一致，会触发 panic。
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }

    /// 检查文件是否可读
    ///
    /// ## Returns
    ///
    /// 如果文件可读返回 `true`，否则返回 `false`
    fn readable(&self) -> bool {
        self.readable
    }

    /// 检查文件是否可写
    ///
    /// ## Returns
    ///
    /// 如果文件可写返回 `true`，否则返回 `false`
    fn writable(&self) -> bool {
        self.writable
    }
}

lazy_static! {
    /// 全局根目录 Inode 实例
    ///
    /// 使用 `lazy_static` 实现全局单例模式，确保整个系统共享同一个根目录 inode。
    /// 该实例在首次访问时初始化，提供对文件系统根目录的访问接口。
    ///
    /// ## 初始化过程
    ///
    /// 1. **文件系统打开**: 通过 `MicroFileSystem::open()` 打开底层文件系统
    /// 2. **根目录获取**: 调用 `MicroFileSystem::root_inode()` 获取根目录 inode
    /// 3. **引用包装**: 将根目录 inode 包装在 `Arc` 中以支持多线程共享访问
    ///
    /// ## 使用场景
    ///
    /// - 文件系统根目录的访问
    /// - 应用程序列表的获取
    /// - 新文件的创建和查找
    /// - 文件系统遍历的起点
    ///
    /// ## 线程安全
    ///
    /// 该实例是线程安全的，多个线程可以同时访问根目录进行文件操作。
    /// 具体的并发控制由底层 Micro-FS 文件系统实现。
    ///
    /// ## 生命周期
    ///
    /// 根目录 inode 的生命周期与系统运行时间相同，在系统启动时初始化，
    /// 在系统关闭时自动清理。
    ///
    /// ## Examples
    ///
    /// ```
    /// use crate::fs::ROOT_INODE;
    ///
    /// // 列出根目录下的所有文件
    /// let files = ROOT_INODE.ls();
    /// for file in files {
    ///     println!("文件: {}", file);
    /// }
    ///
    /// // 在根目录下查找文件
    /// if let Some(file) = ROOT_INODE.find("test.txt") {
    ///     println!("找到文件: test.txt");
    /// }
    /// ```
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = MicroFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(MicroFileSystem::root_inode(&efs))
    };
}

/// 列出应用程序列表
///
/// 打印根目录下所有应用程序的名称，用于系统启动时显示可用的应用程序。
/// 该函数主要用于调试和系统信息显示。
///
/// ## 输出格式
///
/// ```text
/// /**** APPS ****/
/// app1
/// app2
/// app3
/// /**************/
/// ```
///
/// ## 使用场景
///
/// - 系统启动时的应用程序列表显示
/// - 调试时的文件系统内容检查
/// - 用户界面的应用程序选择菜单
///
/// ## 实现说明
///
/// 该函数通过 `ROOT_INODE.ls()` 获取根目录下的所有文件名称，
/// 然后以格式化的方式打印到控制台。
///
/// ## Examples
///
/// ```
/// // 在系统启动时调用
/// list_apps();
/// // 输出: /**** APPS ****/
/// //       initproc
/// //       user_shell
/// //       /**************/
/// ```
pub fn list_apps() {
    println!("/**** APPS ****/");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("/**************/");
}

bitflags! {
    /// 文件打开标志位
    ///
    /// 定义文件打开时的各种标志位，控制文件的打开模式和行为。
    /// 使用 `bitflags` 宏实现，支持标志位的组合和检查。
    ///
    /// ## 标志位说明
    ///
    /// - `RDONLY` - 只读模式，文件只能读取不能写入
    /// - `WRONLY` - 只写模式，文件只能写入不能读取
    /// - `RDWR` - 读写模式，文件既可以读取也可以写入
    /// - `CREATE` - 创建标志，如果文件不存在则创建新文件
    /// - `TRUNC` - 截断标志，如果文件存在则清空文件内容
    ///
    /// ## 组合使用
    ///
    /// 标志位可以组合使用，例如：
    /// - `RDWR | CREATE` - 读写模式，如果文件不存在则创建
    /// - `WRONLY | CREATE | TRUNC` - 只写模式，创建新文件或清空现有文件
    ///
    /// ## 默认行为
    ///
    /// 如果不指定任何标志位（空标志），默认为只读模式。
    pub struct OpenFlags: u32 {
        const RDONLY = 0;
        const WRONLY = 1 << 0;
        const RDWR = 1 << 1;
        const CREATE = 1 << 9;
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// 解析读写权限
    ///
    /// 根据标志位解析文件的读写权限，返回一个元组 `(readable, writable)`。
    ///
    /// ## Returns
    ///
    /// 返回 `(readable, writable)` 元组：
    /// - `readable` - 文件是否可读
    /// - `writable` - 文件是否可写
    ///
    /// ## 解析规则
    ///
    /// - 空标志位：`(true, false)` - 只读模式
    /// - `WRONLY`：`(false, true)` - 只写模式
    /// - `RDWR` 或其他组合：`(true, true)` - 读写模式
    ///
    /// ## Examples
    ///
    /// ```
    /// let flags = OpenFlags::RDWR | OpenFlags::CREATE;
    /// let (readable, writable) = flags.read_write();
    /// assert_eq!(readable, true);
    /// assert_eq!(writable, true);
    /// ```
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

/// 打开文件
///
/// 根据文件名和打开标志位打开文件，返回文件的操作接口。
/// 该函数是文件系统的主要入口点，支持文件的创建、查找和权限控制。
///
/// ## Arguments
///
/// * `name` - 文件名
/// * `flags` - 文件打开标志位
///
/// ## Returns
///
/// - `Some(file)` - 成功打开文件，返回文件操作接口
/// - `None` - 文件不存在且未指定 `CREATE` 标志，或创建失败
///
/// ## 打开流程
///
/// 1. **权限解析**: 根据标志位解析读写权限
/// 2. **文件查找**: 在根目录下查找指定文件
/// 3. **文件创建**: 如果指定 `CREATE` 标志且文件不存在，则创建新文件
/// 4. **文件截断**: 如果指定 `TRUNC` 标志，则清空现有文件内容
/// 5. **接口创建**: 创建 `OSInode` 实例并返回
///
/// ## 标志位处理
///
/// - `CREATE`: 如果文件不存在则创建，如果存在则打开现有文件
/// - `TRUNC`: 清空文件内容，将文件大小设置为 0
/// - 权限标志：控制返回文件的读写权限
///
/// ## 错误处理
///
/// - 文件不存在且未指定 `CREATE` 标志：返回 `None`
/// - 文件创建失败：返回 `None`
/// - 权限不足：返回 `None`（当前实现中未检查权限）
///
/// ## Examples
///
/// ```
/// use crate::fs::{open_file, OpenFlags};
///
/// // 只读打开现有文件
/// let file = open_file("config.txt", OpenFlags::RDONLY);
///
/// // 读写打开，如果不存在则创建
/// let file = open_file("data.txt", OpenFlags::RDWR | OpenFlags::CREATE);
///
/// // 只写打开，清空现有内容
/// let file = open_file("log.txt", OpenFlags::WRONLY | OpenFlags::TRUNC);
/// ```
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode)))
        } else {
            ROOT_INODE
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
        }
    } else {
        ROOT_INODE.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode))
        })
    }
}
