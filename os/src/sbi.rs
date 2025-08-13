#[allow(unused)]
// legacy扩展EID
const SBI_SET_TIMER: usize = 0;
const SBI_CONSOLE_PUTCHAR: usize = 1;
const SBI_CONSOLE_GETCHAR: usize = 2;
const SBI_CLEAR_IPI: usize = 3;
const SBI_SEND_IPI: usize = 4;
const SBI_REMOTE_FENCE_I: usize = 5;
const SBI_REMOTE_SFENCE_VMA: usize = 6;
const SBI_REMOTE_SFENCE_VMA_ASID: usize = 7;
const SBI_SHUTDOWN: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct SbiRet {
    pub error: usize,
    pub value: usize,
}

// 系统复位扩展EID
const SRST_EXTENSION: usize = 0x53525354;
// FID
const SBI_SYSTEM_RESET: usize = 0;

#[repr(usize)]
enum SystemResetType {
    Shutdown = 0,
    ColdReboot = 1,
    WarmReboot = 2,
}

#[repr(usize)]
enum SystemResetReason {
    NoReason = 0,
    SystemFailure = 1,
}

#[inline(always)]
fn sbi_call(eid: usize, fid: usize, arg0: usize, arg1: usize, arg2: usize) -> SbiRet {
    let (error, value);
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") arg0 => error,
            inlateout("a1") arg1 => value,
            in("a2") arg2,
            in("a6") fid,
            in("a7") eid,
            options(nostack, preserves_flags)
        );
    }
    SbiRet { error, value }
}

pub fn console_putchar(c: usize) {
    sbi_call(SBI_CONSOLE_PUTCHAR, 0, c, 0, 0);
}

pub fn shutdown() -> ! {
    let _ = sbi_call(
        SRST_EXTENSION,
        SBI_SYSTEM_RESET,
        SystemResetType::Shutdown as usize,
        SystemResetReason::NoReason as usize,
        0,
    );
    panic!("It should have shutdown !")
}
