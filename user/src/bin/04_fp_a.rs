#![no_std]
#![no_main]
#[macro_use]
extern crate user_lib;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mut acc: f64 = 0.0;
    let mut x: f64 = 1.000_000_1;
    for i in 1..=1_000_000usize {
        acc = (acc + x) * 1.000_000_000_1;
        x += 1.0 / (i as f64 + 0.123);
        if i % 100_000 == 0 {
            println!("[fp_a] step {}", i);
            println!("[fp_a] acc = {}", acc);
        }
    }
    println!("[fp_a] done, acc = {}", acc);
    0
}
