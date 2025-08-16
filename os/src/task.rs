use crate::syscall;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub enum ExceptionType {
    None = -1,
    InstructionMisaligned = 0,
    InstructionFault = 1,
    IllegalInstruction = 2,
    Breakpoint = 3,
    LoadFault = 5,
    StoreMisaligned = 6,
    StoreFault = 7,
    UserEnvCall = 8,
    VirtualSupervisorEnvCall = 10,
    InstructionPageFault = 12,
    LoadPageFault = 13,
    StorePageFault = 15,
    InstructionGuestPageFault = 20,
    LoadGuestPageFault = 21,
    VirtualInstruction = 22,
    StoreGuestPageFault = 23,
    Unknown = 31,
}

impl From<usize> for ExceptionType {
    fn from(value: usize) -> Self {
        match value {
            0 => Self::InstructionMisaligned,
            1 => Self::InstructionFault,
            2 => Self::IllegalInstruction,
            3 => Self::Breakpoint,
            5 => Self::LoadFault,
            6 => Self::StoreMisaligned,
            7 => Self::StoreFault,
            8 => Self::UserEnvCall,
            10 => Self::VirtualSupervisorEnvCall,
            12 => Self::InstructionPageFault,
            13 => Self::LoadPageFault,
            15 => Self::StorePageFault,
            20 => Self::InstructionGuestPageFault,
            21 => Self::LoadGuestPageFault,
            22 => Self::VirtualInstruction,
            23 => Self::StoreGuestPageFault,
            _ => Self::Unknown,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ExceptionInfo {
    pub exception_type: ExceptionType,
    pub fault_address: usize,
    pub instruction_address: usize,
    pub instruction_value: u32,
    pub exception_count: u32,
}

impl Default for ExceptionInfo {
    fn default() -> Self {
        Self {
            exception_type: ExceptionType::None,
            fault_address: 0,
            instruction_address: 0,
            instruction_value: 0,
            exception_count: 0,
        }
    }
}

impl ExceptionInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_exception(
        &mut self,
        exc_type: ExceptionType,
        fault_addr: usize,
        inst_addr: usize,
        inst_val: u32,
    ) {
        self.exception_type = exc_type;
        self.fault_address = fault_addr;
        self.instruction_address = inst_addr;
        self.instruction_value = inst_val;
        self.exception_count += 1;
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SyscallStats {
    pub syscall_counts: [u32; 64],
}

impl Default for SyscallStats {
    fn default() -> Self {
        Self {
            syscall_counts: [0; 64],
        }
    }
}

impl SyscallStats {
    pub fn new() -> Self {
        Self {
            syscall_counts: [0; 64],
        }
    }

    pub fn record_syscall(&mut self, syscall_id: usize) {
        if syscall_id < self.syscall_counts.len() {
            self.syscall_counts[syscall_id] += 1;
        }
    }

    pub fn get_count(&self, syscall_id: usize) -> u32 {
        if syscall_id < self.syscall_counts.len() {
            self.syscall_counts[syscall_id]
        } else {
            0
        }
    }

    pub fn get_total_calls(&self) -> u32 {
        self.syscall_counts.iter().sum()
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TimeStats {
    pub start_time: u64,
    pub end_time: u64,
    pub execution_time: u64,
}

impl Default for TimeStats {
    fn default() -> Self {
        Self {
            start_time: 0,
            end_time: 0,
            execution_time: 0,
        }
    }
}

impl TimeStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, current_time: u64) {
        self.start_time = current_time;
    }

    pub fn end(&mut self, current_time: u64) {
        self.end_time = current_time;
        self.execution_time = self.end_time - self.start_time;
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub enum TaskStatus {
    NotStarted = 0,
    Running = 1,
    Completed = 2,
    Exception = 3,
    Exited = 4,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::NotStarted
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TaskInfo {
    pub task_id: usize,
    pub task_name: [u8; 32],
    pub syscall_stats: SyscallStats,
    pub exception_info: ExceptionInfo,
    pub time_stats: TimeStats,
    pub exit_code: i32,
    pub status: TaskStatus,
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
            syscall_stats: SyscallStats::new(),
            exception_info: ExceptionInfo::new(),
            time_stats: TimeStats::new(),
            exit_code: 0,
            status: TaskStatus::NotStarted,
        }
    }

    pub fn get_name(&self) -> &str {
        let end = self.task_name.iter().position(|&c| c == 0).unwrap_or(32);
        core::str::from_utf8(&self.task_name[..end]).unwrap_or("invalid")
    }

    pub fn record_syscall(&mut self, syscall_id: usize) {
        self.syscall_stats.record_syscall(syscall_id);
    }

    pub fn record_exception(
        &mut self,
        exc_type: ExceptionType,
        fault_addr: usize,
        inst_addr: usize,
        inst_val: u32,
    ) {
        self.exception_info
            .record_exception(exc_type, fault_addr, inst_addr, inst_val);
        self.status = TaskStatus::Exception;
    }

    pub fn set_exit_code(&mut self, code: i32) {
        self.exit_code = code;
        self.status = TaskStatus::Exited;
    }

    pub fn set_completed(&mut self) {
        self.status = TaskStatus::Completed;
    }

    pub fn start_timing(&mut self, current_time: u64) {
        self.time_stats.start(current_time);
        self.status = TaskStatus::Running;
    }

    pub fn end_timing(&mut self, current_time: u64) {
        self.time_stats.end(current_time);
    }
}
