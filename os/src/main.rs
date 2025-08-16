#![no_std]
#![no_main]

mod batch;
mod console;
mod lang_items;
mod log;
mod sbi;
mod stack_trace;
mod sync;
mod syscall;
mod trap;

#[cfg(feature = "board_qemu")]
#[path = "board/qemu.rs"]
mod board;

use core::arch::global_asm;

global_asm!(include_str!("entry.asm"));
global_asm!(include_str!("link_app.S"));

#[unsafe(no_mangle)]
pub fn rust_main() -> ! {
    clear_bss();
    println!("Hello, world!");

    trap::init();
    batch::init();
    log::init();

    batch::run_next_app();
}

fn clear_bss() {
    unsafe extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe {
        (a as *mut u8).write_volatile(0);
    });
}
