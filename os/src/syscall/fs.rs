use crate::batch::{get_current_app_range, get_user_stack_range, run_next_app};
use crate::{print, println};

const FD_STDOUT: usize = 1;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let app_range = get_current_app_range();
    let user_stack_range = get_user_stack_range();

    let buf_start = buf as usize;
    let buf_end = buf_start + len;

    let in_app_range = buf_start >= app_range.0 && buf_end <= app_range.1;
    let in_user_stack_range = buf_start >= user_stack_range.0 && buf_end <= user_stack_range.1;

    if !in_app_range && !in_user_stack_range {
        println!(
            "[kernel] sys_write: buffer out of range [0x{:x}, 0x{:x})",
            buf_start, buf_end
        );
        println!(
            "app_range: [0x{:x}, 0x{:x}), user_stack_range: [0x{:x}, 0x{:x})",
            app_range.0, app_range.1, user_stack_range.0, user_stack_range.1
        );
        sys_exit(-1);
    }

    match fd {
        FD_STDOUT => {
            let slice = unsafe { core::slice::from_raw_parts(buf, len) };
            let str = core::str::from_utf8(slice).unwrap();
            print!("{}", str);
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write: {}!", fd);
        }
    }
}

pub fn sys_exit(xstate: i32) -> ! {
    print!("[kernel] Application exited with code {}", xstate);
    run_next_app();
}
