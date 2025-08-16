use crate::batch::get_current_task_info;
use crate::task::TaskInfo;

pub fn sys_get_taskinfo(ti: *mut u8) -> isize {
    let task_info = get_current_task_info();

    unsafe {
        let src = &task_info as *const TaskInfo as *const u8;
        let size = core::mem::size_of::<TaskInfo>();

        core::ptr::copy_nonoverlapping(src, ti, size);
    }

    0
}
