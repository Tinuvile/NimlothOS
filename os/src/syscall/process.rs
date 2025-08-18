use log::info;

use crate::config::MAX_SYSCALL_COUNT;
use crate::println;
use crate::task::{
    TASK_MANAGER, TaskInfo, exit_current_and_run_next, get_current_task_id,
    get_current_task_status, get_current_task_syscall_times, get_current_task_time,
    suspend_current_and_run_next,
};
use crate::timer::get_time_ms;

pub fn sys_exit(exit_code: i32) -> isize {
    println!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

pub fn sys_get_time() -> isize {
    get_time_ms() as isize
}

pub fn sys_task_info(id: usize, ts: *mut TaskInfo) -> isize {
    info!("[kernel] sys_task_info: id = {}, ts = {:p}", id, ts);
    // let task_info = TaskInfo {
    //     id: get_current_task_id(),
    //     status: get_current_task_status(),
    //     call: get_current_task_syscall_times(),
    //     times: get_current_task_time(),
    // };
    // unsafe {
    //     let src_ptr = &task_info as *const TaskInfo as *const u8;
    //     let dst_ptr = ts as *mut u8;
    //     let size = core::mem::size_of::<TaskInfo>();

    //     for i in 0..size {
    //         dst_ptr
    //             .add(i)
    //             .write_volatile(src_ptr.add(i).read_volatile());
    //     }
    // }
    0
}
