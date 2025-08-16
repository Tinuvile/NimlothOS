use core::{
    arch::asm,
    ptr::{self, null},
};

use crate::println;

pub unsafe fn print_stack_trace() {
    let mut fp: *const usize;
    asm!("mv {}, s0", out(reg) fp);

    println!("=== Call Stack Trace Tool ===");
    while fp != ptr::null() {
        let saved_ra = *fp.sub(1);
        let saved_fp = *fp.sub(2);

        println!("0x{:016x} (0x{:016x})", saved_ra, saved_fp);

        fp = saved_fp as *const usize;
    }
    println!("=== End of Call Stack Trace ===");
}
