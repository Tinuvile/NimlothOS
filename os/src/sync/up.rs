//! # 单处理器安全单元
//!
//! 提供在单处理器环境下的线程安全共享可变数据结构。

use core::cell::{RefCell, RefMut};

/// 单处理器安全单元 (Uniprocessor Safe Cell)
///
/// 在单处理器系统中，通过禁用中断可以确保原子性操作。
/// `UPSafeCell<T>` 是 `RefCell<T>` 的一个封装，用于在内核中
/// 安全地共享可变数据。
///
/// ## 使用场景
///
/// - 内核全局状态管理
/// - 任务管理器等需要在中断处理程序和普通代码之间共享的数据
/// - 需要内部可变性的静态数据结构
///
/// ## Safety
///
/// 该结构体实现了 `Sync`，但这需要调用者保证：
/// - 在访问数据时禁用中断，确保不会被中断处理程序打断
/// - 在单处理器环境下使用
///
/// ## Examples
///
/// ```rust
/// use crate::sync::UPSafeCell;
///
/// static GLOBAL_DATA: UPSafeCell<Vec<i32>> = unsafe { UPSafeCell::new(Vec::new()) };
///
/// // 在中断禁用的情况下访问数据
/// let mut data = GLOBAL_DATA.exclusive_access();
/// data.push(42);
/// ```
pub struct UPSafeCell<T> {
    /// 内部的 RefCell，提供运行时借用检查
    inner: RefCell<T>,
}

/// 为 `UPSafeCell<T>` 实现 `Sync` trait
///
/// 这是一个 unsafe 实现，因为 `RefCell<T>` 本身不是 `Sync` 的。
/// 在单处理器系统中，只要确保访问时禁用了中断，就可以安全地
/// 在多个执行上下文之间共享 `UPSafeCell`。
unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    /// 创建一个新的 `UPSafeCell`
    ///
    /// ## Arguments
    ///
    /// * `value` - 要封装的初始值
    ///
    /// ## Returns
    ///
    /// 返回一个新的 `UPSafeCell` 实例
    ///
    /// ## Safety
    ///
    /// 调用者必须确保：
    /// - 在单处理器环境下使用
    /// - 在访问时正确禁用中断
    pub unsafe fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }

    /// 获取对内部数据的独占可变引用
    ///
    /// 这个方法会动态检查借用规则，如果已经有其他借用存在，
    /// 会导致 panic。在正确的使用模式下（访问时禁用中断），
    /// 这种情况不应该发生。
    ///
    /// ## Returns
    ///
    /// 返回一个 `RefMut<'_, T>`，允许可变访问内部数据
    ///
    /// ## Panics
    ///
    /// 如果内部数据已经被借用（通过其他的 `exclusive_access` 调用），
    /// 此方法会 panic
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}
