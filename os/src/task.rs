#[repr(C)]
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: usize,
    pub task_name: [u8; 32],
}

impl TaskInfo {
    pub fn new(id: usize, name: &str) -> Self {
        let mut task_name = [0u8; 32];
        let name_bytes = name.as_bytes();
        let copy_len = core::cmp::min(name_bytes.len(), 31);
        task_name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

        TaskInfo {
            task_id: id,
            task_name,
        }
    }
}
