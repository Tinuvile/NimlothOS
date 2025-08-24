#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{exit, fork, pid, sleep, time, wait};

static NUM: usize = 30;

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    for _ in 0..NUM {
        let _pid = fork();
        if _pid == 0 {
            let current_time = time();
            let sleep_length =
                (current_time as i32 as isize) * (current_time as i32 as isize) % 1000 + 1000;
            println!("pid {} sleep for {} ms", pid(), sleep_length);
            sleep(sleep_length as usize);
            println!("pid {} OK!", pid());
            exit(0);
        }
    }

    let mut exit_code: i32 = 0;
    for _ in 0..NUM {
        assert!(wait(&mut exit_code) > 0);
        assert_eq!(exit_code, 0);
    }
    assert!(wait(&mut exit_code) < 0);
    println!("forktest2 test passed!");
    0
}
