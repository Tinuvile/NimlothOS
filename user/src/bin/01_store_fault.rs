#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("Info Test StoreFault, we will insert an invalid store operation...");
    println!("Kernel should kill this application");
    unsafe {
        core::ptr::null_mut::<u8>().write_volatile(0);
    }
    0
}
