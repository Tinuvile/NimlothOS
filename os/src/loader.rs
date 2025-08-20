//! # 应用程序加载器模块
//!
//! 提供从内核镜像中加载用户应用程序的功能。用户应用程序在构建时被链接到
//! 内核镜像中，运行时通过此模块提供的接口进行访问和加载。
//!
//! ## 设计原理
//!
//! ### 应用程序嵌入
//!
//! 用户应用程序通过构建脚本 (`build.rs`) 和汇编文件 (`link_app.S`) 被嵌入到
//! 内核镜像中。构建过程会：
//!
//! 1. 编译所有用户应用程序为独立的二进制文件
//! 2. 生成包含应用程序数据的汇编代码
//! 3. 将汇编代码链接到内核镜像中
//!
//! ### 内存布局
//!
//! 嵌入的应用程序数据在内核镜像中的布局：
//!
//! ```text
//! _num_app symbol location:
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Number of Apps                          │
//! │                       (8 bytes)                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   App 0 Start Address                       │
//! │                       (8 bytes)                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   App 1 Start Address                       │
//! │                       (8 bytes)                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                          ...                                │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   App N End Address                         │
//! │                       (8 bytes)                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │                       App 0 Data                            │
//! │                       (Variable)                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │                       App 1 Data                            │
//! │                       (Variable)                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │                          ...                                │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 安全注意事项
//!
//! - 所有内存访问都使用 `read_volatile()` 避免编译器优化
//! - 应用程序 ID 边界检查防止越界访问
//! - 使用 `unsafe` 代码块明确标识不安全操作
//!
//! ## 使用示例
//!
//! ```rust
//! // 获取应用程序数量
//! let app_count = get_num_app();
//! println!("Found {} applications", app_count);
//!
//! // 加载特定应用程序
//! for i in 0..app_count {
//!     let app_data = get_app_data(i);
//!     println!("App {}: {} bytes", i, app_data.len());
//!     // 将应用程序数据加载到内存中...
//! }
//! ```

/// 获取嵌入的应用程序数量
///
/// 从内核镜像中读取应用程序数量，该数量在构建时确定并嵌入到镜像中。
///
/// ## 实现细节
///
/// 函数通过外部符号 `_num_app` 访问应用程序数量。该符号由构建脚本生成的
/// 汇编代码定义，指向一个包含应用程序数量的内存位置。
///
/// ## Returns
///
/// 嵌入到内核镜像中的应用程序数量
///
/// ## Safety
///
/// 此函数使用 `unsafe` 代码访问外部符号和原始内存，但通过以下方式确保安全：
/// - 使用 `read_volatile()` 防止编译器优化
/// - `_num_app` 符号由构建系统保证有效
/// - 读取的是构建时确定的常量值
///
/// ## Examples
///
/// ```rust
/// let count = get_num_app();
/// assert!(count > 0, "No applications found");
/// ```
pub fn get_num_app() -> usize {
    unsafe extern "C" {
        fn _num_app();
    }
    unsafe { (_num_app as usize as *const usize).read_volatile() }
}

/// 获取指定应用程序的二进制数据
///
/// 从内核镜像中提取指定 ID 的应用程序二进制数据，返回包含完整 ELF 文件的字节切片。
///
/// ## Arguments
///
/// * `app_id` - 应用程序标识符，范围为 `[0, get_num_app())`
///
/// ## Returns
///
/// 指向应用程序二进制数据的静态字节切片，包含完整的 ELF 文件内容
///
/// ## 实现原理
///
/// 1. **获取地址表**: 从 `_num_app` 符号开始读取地址数组
/// 2. **边界检查**: 验证 `app_id` 在有效范围内
/// 3. **计算范围**: 使用相邻两个地址计算应用程序数据的起始和结束位置
/// 4. **创建切片**: 基于计算的地址范围创建字节切片
///
/// ## Panics
///
/// 如果 `app_id` 大于等于应用程序总数，函数会触发 panic
///
/// ## Safety
///
/// 函数使用大量 `unsafe` 代码，但通过以下方式确保内存安全：
/// - 应用程序 ID 边界检查
/// - 地址计算基于构建时生成的有效数据
/// - 所有内存访问都在内核镜像的有效范围内
///
/// ## Examples
///
/// ```rust
/// let app_count = get_num_app();
/// for i in 0..app_count {
///     let app_data = get_app_data(i);
///     println!("Application {}: {} bytes", i, app_data.len());
///     
///     // 验证 ELF 魔数
///     if app_data.len() >= 4 {
///         assert_eq!(&app_data[0..4], b"\x7fELF");
///     }
/// }
/// ```
///
/// ## 内存布局示意
///
/// ```text
/// _num_app -> [应用数量] [app0_start] [app1_start] ... [appN_end]
///                        ↓
///                     [应用程序0数据] [应用程序1数据] ...
/// ```
pub fn get_app_data(app_id: usize) -> &'static [u8] {
    unsafe extern "C" {
        fn _num_app();
    }
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app();
    let app_start = unsafe { core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1) };
    assert!(app_id < num_app);
    unsafe {
        core::slice::from_raw_parts(
            app_start[app_id] as *const u8,
            app_start[app_id + 1] - app_start[app_id],
        )
    }
}
