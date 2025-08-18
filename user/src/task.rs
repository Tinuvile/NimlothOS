use crate::config::MAX_SYSCALL_COUNT;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub enum TaskStatus {
    UnInit,
    Ready,
    Running,
    Exited,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct SyscallInfo {
    pub id: usize,
    pub times: usize,
}

#[derive(Debug)]
#[repr(C)]
pub struct TaskInfo {
    pub id: usize,
    pub status: TaskStatus,
    pub call: [SyscallInfo; MAX_SYSCALL_COUNT],
    pub times: usize,
}
