//! # 应用程序加载器模块
//!
//! 负责加载用户应用程序到内存并为其创建执行上下文。
//! 支持多个应用程序的静态链接和动态加载，为每个应用程序分配独立的栈空间。
//!
//! ## 功能特性
//!
//! - **静态链接**: 应用程序在编译时被链接到内核镜像中
//! - **内存隔离**: 每个应用程序有独立的内存区域和栈空间  
//! - **上下文创建**: 为每个应用程序创建初始执行上下文
//! - **批量加载**: 一次性加载所有应用程序到指定内存位置
//!
//! ## 内存布局
//!
//! ```text
//! 高地址
//! ┌─────────────────────┐
//! │   内核栈 (App N-1)   │ <- 每个应用独立的内核栈
//! ├─────────────────────┤
//! │       ...           │
//! ├─────────────────────┤  
//! │   内核栈 (App 0)     │
//! ├─────────────────────┤
//! │   用户栈 (App N-1)   │ <- 每个应用独立的用户栈
//! ├─────────────────────┤
//! │       ...           │
//! ├─────────────────────┤
//! │   用户栈 (App 0)     │
//! ├─────────────────────┤
//! │ 应用程序 N-1 代码     │ <- APP_BASE_ADDRESS + (N-1) * APP_SIZE_LIMIT
//! ├─────────────────────┤
//! │       ...           │  
//! ├─────────────────────┤
//! │ 应用程序 0 代码       │ <- APP_BASE_ADDRESS
//! └─────────────────────┘
//! 低地址
//! ```

use crate::config::*;
use crate::trap::TrapContext;
use core::arch::asm;

/// 内核栈结构
///
/// 为每个应用程序分配的内核栈，用于处理该应用程序的系统调用和中断。
/// 栈按 4KB 页面对齐，确保内存管理的兼容性。
#[repr(align(4096))]
#[derive(Clone, Copy)]
struct KernelStack {
    /// 栈数据区域，大小为 [`KERNEL_STACK_SIZE`] (8KB)
    data: [u8; KERNEL_STACK_SIZE],
}

/// 用户栈结构
///
/// 为每个应用程序分配的用户栈，用于应用程序在用户态的函数调用和局部变量。
/// 栈按 4KB 页面对齐，确保内存管理的兼容性。
#[repr(align(4096))]
#[derive(Clone, Copy)]
struct UserStack {
    /// 栈数据区域，大小为 [`USER_STACK_SIZE`] (8KB)
    data: [u8; USER_STACK_SIZE],
}

/// 所有应用程序的内核栈数组
///
/// 静态分配，每个应用程序对应一个内核栈。
/// 内核栈用于处理来自该应用程序的系统调用、中断和异常。
static KERNEL_STACK: [KernelStack; MAX_APP_NUM] = [KernelStack {
    data: [0; KERNEL_STACK_SIZE],
}; MAX_APP_NUM];

/// 所有应用程序的用户栈数组
///
/// 静态分配，每个应用程序对应一个用户栈。
/// 用户栈供应用程序在用户态执行时使用。
static USER_STACK: [UserStack; MAX_APP_NUM] = [UserStack {
    data: [0; USER_STACK_SIZE],
}; MAX_APP_NUM];

impl KernelStack {
    /// 获取内核栈栈顶指针
    ///
    /// 计算栈的最高地址（栈顶），栈从高地址向低地址增长。
    ///
    /// ## Returns
    ///
    /// 返回栈顶的虚拟地址
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + KERNEL_STACK_SIZE
    }

    /// 将陷阱上下文压入内核栈
    ///
    /// 在内核栈顶分配空间存储陷阱上下文，并返回陷阱上下文的地址。
    /// 这个地址会被用作任务上下文的栈指针。
    ///
    /// ## Arguments
    ///
    /// * `trap_cx` - 要压入栈的陷阱上下文
    ///
    /// ## Returns
    ///
    /// 返回压入栈的陷阱上下文的地址
    ///
    /// ## Memory Layout
    ///
    /// ```text
    /// 栈顶 (高地址)
    /// ┌─────────────────────┐
    /// │   TrapContext       │ <- 返回的地址
    /// ├─────────────────────┤
    /// │   可用栈空间          │
    /// └─────────────────────┘
    /// 栈底 (低地址)
    /// ```
    ///
    /// ## Safety
    ///
    /// 使用 `unsafe` 代码直接操作内存指针，假设栈空间足够大。
    pub fn push_context(&self, trap_cx: TrapContext) -> usize {
        let trap_cx_ptr = (self.get_sp() - core::mem::size_of::<TrapContext>()) as *mut TrapContext;
        unsafe {
            *trap_cx_ptr = trap_cx;
        }
        trap_cx_ptr as usize
    }
}

impl UserStack {
    /// 获取用户栈栈顶指针
    ///
    /// 计算栈的最高地址（栈顶），栈从高地址向低地址增长。
    ///
    /// ## Returns
    ///
    /// 返回栈顶的虚拟地址，用于设置应用程序的初始栈指针
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + USER_STACK_SIZE
    }
}

/// 加载所有应用程序到内存
///
/// 从链接器生成的应用程序数据中提取每个应用程序的二进制代码，
/// 并将其加载到预分配的内存区域中。
///
/// ## 加载流程
///
/// 1. **读取应用程序信息**: 从 `_num_app` 符号获取应用程序数量和地址表
/// 2. **清零目标内存**: 清理每个应用程序的内存区域
/// 3. **复制应用程序**: 将应用程序二进制数据复制到目标地址
/// 4. **指令缓存同步**: 执行 `fence.i` 确保指令缓存一致性
///
/// ## 内存布局
///
/// 应用程序按固定间隔加载：
/// - App 0: `APP_BASE_ADDRESS`
/// - App 1: `APP_BASE_ADDRESS + APP_SIZE_LIMIT`  
/// - App N: `APP_BASE_ADDRESS + N * APP_SIZE_LIMIT`
///
/// ## Safety
///
/// 使用多个 `unsafe` 操作：
/// - 访问链接器符号 `_num_app`
/// - 创建原始指针切片
/// - 直接内存复制操作
/// - 执行汇编指令 `fence.i`
///
/// ## Note
///
/// - 必须在系统初始化早期调用，在启动任务调度之前
/// - 每个应用程序的大小不能超过 [`APP_SIZE_LIMIT`]
/// - 指令缓存同步确保 CPU 能正确执行新加载的代码
pub fn load_apps() {
    unsafe extern "C" {
        fn _num_app();
    }
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app();
    let app_start = unsafe { core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1) };

    // 为每个应用程序分配内存并加载
    for i in 0..num_app {
        let base_i = get_base_i(i);

        // 清零应用程序内存区域
        (base_i..base_i + APP_SIZE_LIMIT).for_each(|addr| unsafe {
            (addr as *mut u8).write_volatile(0);
        });

        // 复制应用程序二进制数据
        let app_src = unsafe {
            core::slice::from_raw_parts(app_start[i] as *const u8, app_start[i + 1] - app_start[i])
        };
        let app_dst = unsafe { core::slice::from_raw_parts_mut(base_i as *mut u8, app_src.len()) };
        app_dst.copy_from_slice(app_src);
    }

    // 指令缓存同步，确保新加载的指令对 CPU 可见
    unsafe {
        asm!("fence.i");
    }
}

/// 获取应用程序数量
///
/// 从链接器生成的 `_num_app` 符号读取系统中包含的应用程序总数。
///
/// ## Returns
///
/// 返回应用程序数量
///
/// ## Implementation
///
/// `_num_app` 符号由构建脚本生成，包含应用程序数量和地址表。
/// 第一个 `usize` 值是应用程序数量。
///
/// ## Safety
///
/// 使用 `unsafe` 代码访问链接器符号和原始指针。
pub fn get_num_app() -> usize {
    unsafe extern "C" {
        fn _num_app();
    }
    unsafe { (_num_app as usize as *const usize).read_volatile() }
}

/// 计算应用程序的加载基地址
///
/// 根据应用程序 ID 计算其在内存中的加载地址。
/// 每个应用程序占用 [`APP_SIZE_LIMIT`] 大小的连续内存空间。
///
/// ## Arguments
///
/// * `app_id` - 应用程序 ID，从 0 开始编号
///
/// ## Returns
///
/// 返回应用程序的加载基地址
///
/// ## Formula
///
/// `base_address = APP_BASE_ADDRESS + app_id * APP_SIZE_LIMIT`
fn get_base_i(app_id: usize) -> usize {
    APP_BASE_ADDRESS + app_id * APP_SIZE_LIMIT
}

/// 初始化应用程序上下文
///
/// 为指定的应用程序创建初始陷阱上下文，并将其压入对应的内核栈。
/// 返回陷阱上下文的地址，用于任务调度。
///
/// ## Arguments
///
/// * `app_id` - 应用程序 ID，从 0 开始编号
///
/// ## Returns
///
/// 返回陷阱上下文在内核栈中的地址，用作任务上下文的栈指针
///
/// ## 上下文配置
///
/// - **程序入口**: 应用程序的加载基地址
/// - **用户栈指针**: 对应用户栈的栈顶地址
/// - **特权级**: 用户态 (User mode)
/// - **寄存器状态**: 全部初始化为 0
///
/// ## Usage
///
/// 该函数在系统初始化时为每个应用程序调用一次，
/// 创建的上下文地址会被存储在任务控制块中。
pub fn init_app_cx(app_id: usize) -> usize {
    KERNEL_STACK[app_id].push_context(TrapContext::app_init_context(
        get_base_i(app_id),          // 程序入口地址
        USER_STACK[app_id].get_sp(), // 用户栈指针
    ))
}
