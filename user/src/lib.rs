#![no_std]
#![no_main]
#![feature(linkage)]

use syscall::*;

#[macro_use]
pub mod console;
mod lang_items;
mod syscall;

#[repr(C)]
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: usize,
    pub task_name: [u8; 32],
    pub syscall_stats: [u32; 64],
    pub exception_info: [u32; 32],
    pub time_stats: [u64; 3],
    pub exit_code: i32,
    pub status: u32,
}

impl TaskInfo {
    pub fn get_name(&self) -> &str {
        let end = self.task_name.iter().position(|&c| c == 0).unwrap_or(32);
        core::str::from_utf8(&self.task_name[..end]).unwrap_or("invalid")
    }

    pub fn get_syscall_count(&self, syscall_id: usize) -> u32 {
        if syscall_id < self.syscall_stats.len() {
            self.syscall_stats[syscall_id] as u32
        } else {
            0
        }
    }

    pub fn get_total_calls(&self) -> u32 {
        self.syscall_stats.iter().sum()
    }

    pub fn get_execution_time(&self) -> u64 {
        self.time_stats[2]
    }
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    clear_bss();
    exit(main());
    panic!("unreachable after sys_exit!");
}

#[linkage = "weak"]
#[unsafe(no_mangle)]
fn main() -> i32 {
    panic!("Cannot find main function!");
}

fn clear_bss() {
    unsafe extern "C" {
        safe fn start_bss();
        safe fn end_bss();
    }
    (start_bss as usize..end_bss as usize).for_each(|a| unsafe {
        (a as *mut u8).write_volatile(0);
    });
}

pub fn write(fd: usize, buf: &[u8]) -> isize {
    sys_write(fd, buf)
}

pub fn exit(exit_code: i32) -> isize {
    sys_exit(exit_code)
}

pub fn get_taskinfo() -> TaskInfo {
    let mut ti = TaskInfo {
        task_id: 0,
        task_name: [0; 32],
        syscall_stats: [0; 64],
        exception_info: [0; 32],
        time_stats: [0; 3],
        exit_code: 0,
        status: 0,
    };

    sys_get_taskinfo(&mut ti as *mut TaskInfo as *mut u8);

    ti
}
