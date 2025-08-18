use super::context::TaskContext;

#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
    pub task_start_time: isize,
}

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Uninit,
    Ready,
    Running,
    Exited,
}
