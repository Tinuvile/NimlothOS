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
use super::println;
use alloc::vec::Vec;
use lazy_static::*;

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
        safe fn _num_app();
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
        safe fn _num_app();
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

lazy_static! {
    /// 应用程序名称缓存
    ///
    /// 延迟初始化的静态变量，包含所有嵌入应用程序的名称列表。应用程序名称在构建时
    /// 确定并以 null 终止的字符串形式嵌入到内核镜像中。
    ///
    /// ## 设计原理
    ///
    /// ### 名称存储格式
    ///
    /// 应用程序名称通过外部符号 `_app_names` 访问，数据格式为连续的 null 终止字符串：
    ///
    /// ```text
    /// _app_names symbol location:
    /// ┌─────────────────────────────────────────────────────────────┐
    /// │ "app0" │ \0 │ "app1" │ \0 │ "app2" │ \0 │ ... │ "appN" │ \0 │
    /// └─────────────────────────────────────────────────────────────┘
    /// ```
    ///
    /// ### 初始化过程
    ///
    /// 1. **获取数量**: 通过 `get_num_app()` 确定应用程序总数
    /// 2. **遍历名称**: 从 `_app_names` 开始逐个读取 null 终止的字符串
    /// 3. **创建切片**: 为每个名称创建字符串切片
    /// 4. **UTF-8 转换**: 将字节切片转换为有效的 UTF-8 字符串
    /// 5. **构建向量**: 将所有名称存储在向量中供后续查询
    ///
    /// ## 内存安全
    ///
    /// - 所有内存读取使用 `read_volatile()` 防止编译器优化
    /// - 字符串切片基于构建时生成的有效数据
    /// - UTF-8 转换经过验证，确保字符串有效性
    ///
    /// ## 性能特性
    ///
    /// - **延迟初始化**: 仅在首次访问时解析名称数据
    /// - **零拷贝**: 直接引用内核镜像中的字符串数据
    /// - **一次性成本**: 初始化后的访问为 O(1) 复杂度
    static ref APP_NAMES: Vec<&'static str> = {
        let num_app = get_num_app();
        unsafe extern "C" {
            safe fn _app_names();
        }
        let mut start = _app_names as usize as *const u8;
        let mut v = Vec::new();
        unsafe {
            for _ in 0..num_app {
                let mut end = start;
                while end.read_volatile() != b'\0' {
                    end = end.add(1)
                }
                let slice = core::slice::from_raw_parts(start, end as usize - start as usize);
                let str = core::str::from_utf8(slice).unwrap();
                v.push(str);
                start = end.add(1);
            }
        }
        v
    };
}

/// 根据应用程序名称获取二进制数据
///
/// 通过应用程序名称查找并返回对应的二进制数据。此函数提供了比基于 ID 的
/// `get_app_data()` 更友好的访问方式，支持通过字符串名称直接定位应用程序。
///
/// ## Arguments
///
/// * `name` - 要查找的应用程序名称，必须与构建时嵌入的名称完全匹配（区分大小写）
///
/// ## Returns
///
/// * `Some(&'static [u8])` - 如果找到匹配的应用程序，返回指向其二进制数据的切片
/// * `None` - 如果没有找到匹配名称的应用程序
///
/// ## 实现细节
///
/// 1. **名称查找**: 在 `APP_NAMES` 向量中线性搜索匹配的名称
/// 2. **ID 映射**: 将找到的名称索引作为应用程序 ID
/// 3. **数据获取**: 调用 `get_app_data()` 获取对应的二进制数据
///
/// ## 性能考虑
///
/// - **时间复杂度**: O(n)，其中 n 是应用程序数量
/// - **空间复杂度**: O(1)，不额外分配内存
/// - **首次访问**: 触发 `APP_NAMES` 的延迟初始化
///
/// ## Examples
///
/// ```rust
/// // 查找特定应用程序
/// if let Some(app_data) = get_app_data_by_name("hello_world") {
///     println!("Found hello_world: {} bytes", app_data.len());
///     // 加载并执行应用程序...
/// } else {
///     println!("Application 'hello_world' not found");
/// }
///
/// // 批量查找多个应用程序
/// let apps_to_load = ["init", "shell", "user_program"];
/// for app_name in apps_to_load.iter() {
///     match get_app_data_by_name(app_name) {
///         Some(data) => println!("Loaded {}: {} bytes", app_name, data.len()),
///         None => println!("Warning: {} not found", app_name),
///     }
/// }
/// ```
///
/// ## 使用场景
///
/// - **配置驱动加载**: 基于配置文件中的应用程序名称进行加载
/// - **交互式选择**: 用户通过名称选择要运行的应用程序
/// - **依赖解析**: 根据应用程序声明的依赖名称加载相关程序
/// - **调试辅助**: 开发时通过可读名称快速定位特定应用程序
#[allow(unused)]
pub fn get_app_data_by_name(name: &str) -> Option<&'static [u8]> {
    let num_app = get_num_app();
    (0..num_app)
        .find(|&i| APP_NAMES[i] == name)
        .map(get_app_data)
}

/// 列出所有可用的应用程序名称
///
/// 打印所有嵌入到内核镜像中的应用程序名称列表，主要用于调试、诊断和
/// 用户界面显示。输出格式为带有装饰边框的列表。
///
/// ## 输出格式
///
/// ```text
/// /**** APPS ****
/// app_name_1
/// app_name_2
/// app_name_3
/// ...
/// **************/
/// ```
///
/// ## 使用场景
///
/// ### 系统诊断
/// - **启动检查**: 验证所有预期的应用程序都已正确嵌入
/// - **构建验证**: 确认构建过程正确处理了所有应用程序
/// - **调试辅助**: 快速查看可用的应用程序列表
///
/// ### 用户交互
/// - **菜单显示**: 为用户提供可选择的应用程序列表
/// - **命令行帮助**: 显示可执行的程序名称
/// - **系统信息**: 作为系统状态报告的一部分
///
/// ## 实现特性
///
/// - **延迟初始化**: 首次调用时触发 `APP_NAMES` 的初始化
/// - **零拷贝**: 直接打印内核镜像中的字符串数据
/// - **格式化输出**: 提供清晰的视觉分隔，便于阅读
///
/// ## Examples
///
/// ```rust
/// // 系统启动时显示可用应用程序
/// println!("System initialized with the following applications:");
/// list_apps();
///
/// // 在交互式 shell 中使用
/// fn show_help() {
///     println!("Available commands:");
///     list_apps();
///     println!("Use 'run <app_name>' to execute an application.");
/// }
/// ```
///
/// ## 性能注意事项
///
/// - **输出成本**: 每个应用程序名称需要一次 `println!` 调用
/// - **初始化成本**: 首次调用可能触发名称数据的解析
/// - **适用场景**: 主要用于调试和低频率的用户交互
#[allow(unused)]
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in APP_NAMES.iter() {
        println!("{}", app);
    }
    println!("**************/");
}
