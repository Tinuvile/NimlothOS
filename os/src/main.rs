#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

#[macro_use]
mod config;
mod console;
mod lang_items;
mod loader;
mod log;
mod mm;
mod sbi;
mod stack_trace;
mod sync;
mod syscall;
mod task;
mod timer;
mod trap;

#[path = "board/qemu.rs"]
mod board;

use core::arch::global_asm;

extern crate alloc;

global_asm!(include_str!("entry.asm"));
global_asm!(include_str!("link_app.S"));

#[unsafe(no_mangle)]
pub fn rust_main() -> ! {
    clear_bss();
    log::init();
    info!("[kernel] Hello, world!");

    trap::init();
    loader::load_apps();
    trap::enable_timer_interrupt();
    timer::set_next_trigger();
    task::run_first_task();

    panic!("Unreachable in rust_main!");
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
