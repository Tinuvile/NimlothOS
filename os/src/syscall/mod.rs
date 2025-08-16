use fs::*;
use process::*;

mod fs;
mod process;

const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_GET_TASKINFO: usize = 410;

pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    crate::batch::record_syscall(syscall_id);

    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_GET_TASKINFO => sys_get_taskinfo(args[0] as *mut u8),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
