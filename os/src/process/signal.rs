//! # 进程信号（Signals）模块
//!
//! 提供类 Unix 信号的基础表示与默认动作表，用于进程间/内核向进程投递异步事件。
//! 在本实现中，信号集合以位集合（bitflags）表示，支持屏蔽、待决、默认处理与
//! 用户自定义处理（参见 `process/mod.rs` 中的处理流程）。
//!
//! ## 组成
//! - [`SignalFlags`]：信号位集合类型
//! - [`SignalAction`]：单个信号的处理动作（用户态处理入口与掩码）
//! - [`SignalActions`]：全表（索引 0..=MAX_SIG）
//!
//! ## 常见语义
//! - 致命/错误类信号转为负退出码（见 [`SignalFlags::check_error`]）
//! - 控制类信号（`SIGSTOP`/`SIGCONT`）由内核内建处理
//! - 其余可捕捉信号可由用户程序通过 `sigaction` 自定义处理
//!
use bitflags::*;

/// 支持的最大信号编号（含）
///
/// 本实现支持 0..=MAX_SIG 共 32 个编号槽位，对应的位掩码使用 `1 << signum`。
pub const MAX_SIG: usize = 31;

bitflags! {
    /// 信号位集合
    ///
    /// 每一位对应一个信号，结合 `insert/contains/remove` 操作可维护待决集合、
    /// 屏蔽集合等。数值与传统 Unix 信号编号保持一致（部分信号为兼容保留）。
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct SignalFlags: u32 {
        const SIGDEF = 1;
        const SIGHUP = 1 << 1;
        const SIGINT = 1 << 2;
        const SIGQUIT = 1 << 3;
        const SIGILL = 1 << 4;
        const SIGTRAP = 1 << 5;
        const SIGABRT = 1 << 6;
        const SIGBUS = 1 << 7;
        const SIGFPE = 1 << 8;
        const SIGKILL = 1 << 9;
        const SIGUSR1 = 1 << 10;
        const SIGSEGV = 1 << 11;
        const SIGUSR2 = 1 << 12;
        const SIGPIPE = 1 << 13;
        const SIGALRM = 1 << 14;
        const SIGTERM = 1 << 15;
        const SIGSTKFLT = 1 << 16;
        const SIGCHLD = 1 << 17;
        const SIGCONT = 1 << 18;
        const SIGSTOP = 1 << 19;
        const SIGTSTP = 1 << 20;
        const SIGTTIN = 1 << 21;
        const SIGTTOU = 1 << 22;
        const SIGURG = 1 << 23;
        const SIGXCPU = 1 << 24;
        const SIGXFSZ = 1 << 25;
        const SIGVTALRM = 1 << 26;
        const SIGPROF = 1 << 27;
        const SIGWINCH = 1 << 28;
        const SIGIO = 1 << 29;
        const SIGPWR = 1 << 30;
        const SIGSYS = 1 << 31;
    }
}

impl SignalFlags {
    /// 将集合中的致命/错误类信号映射为标准退出码与原因
    ///
    /// 若集合包含以下任意一个信号，则返回对应的 `(负退出码, 静态说明)`：
    /// - `SIGINT`/`SIGILL`/`SIGABRT`/`SIGFPE`/`SIGKILL`/`SIGSEGV`
    ///
    /// 否则返回 `None`，表示不属于错误类（可能是可捕捉或控制类信号）。
    pub fn check_error(&self) -> Option<(i32, &'static str)> {
        if self.contains(Self::SIGINT) {
            Some((-2, "Killed, SIGINT=2"))
        } else if self.contains(Self::SIGILL) {
            Some((-4, "Illegal Instruction, SIGILL=4"))
        } else if self.contains(Self::SIGABRT) {
            Some((-6, "Aborted, SIGABRT=6"))
        } else if self.contains(Self::SIGFPE) {
            Some((-8, "Erroneous Arithmetic Operation, SIGFPE=8"))
        } else if self.contains(Self::SIGKILL) {
            Some((-9, "Killed, SIGKILL=9"))
        } else if self.contains(Self::SIGSEGV) {
            Some((-11, "Segmentation Fault, SIGSEGV=11"))
        } else {
            None
        }
    }
}

/// 用户态信号处理动作
///
/// - `handler`：用户态处理函数入口（0 表示采用默认动作）
/// - `mask`：进入处理程序期间额外屏蔽的信号集合
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalAction {
    pub handler: usize,
    pub mask: SignalFlags,
}

impl Default for SignalAction {
    /// 默认处理：不设置用户处理入口，屏蔽集合按内核预设（可调整）
    fn default() -> Self {
        Self {
            handler: 0,
            mask: SignalFlags::from_bits(40).unwrap(),
        }
    }
}

/// 全部信号的处理动作表（索引 0..=MAX_SIG）
#[derive(Clone)]
pub struct SignalActions {
    pub table: [SignalAction; MAX_SIG + 1],
}

impl Default for SignalActions {
    /// 初始化为全默认动作
    fn default() -> Self {
        Self {
            table: [SignalAction::default(); MAX_SIG + 1],
        }
    }
}
