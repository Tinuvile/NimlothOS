#![no_std]
#![no_main]
#[macro_use]
extern crate user_lib;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut acc: f64 = 1.0;
    let mut y: f64 = 0.999_999_7;
    for i in 1..=1_000_000usize {
        acc = acc * y + 3.141592653589793 / (i as f64 + 0.5);
        y *= 0.999_999_9;
        if i % 100_000 == 0 {
            println!("[fp_b] step {}", i);
            println!("[fp_b] acc = {}", acc);
        }
    }
    println!("[fp_b] done, acc = {}", acc);
    0
}
