#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::pid;

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    println!("pid {}: Hello world from user mode program!", pid());
    0
}
