#![no_std]
#![no_main]
#![feature(linkage)]

use syscall::*;

#[macro_use]
pub mod console;
mod lang_items;
mod syscall;

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

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ExceptionInfo {
    pub exception_type: ExceptionType,
    pub fault_address: usize,
    pub instruction_address: usize,
    pub instruction_value: u32,
    pub exception_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SyscallStats {
    pub syscall_counts: [u32; 64],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TimeStats {
    pub start_time: u64,
    pub end_time: u64,
    pub execution_time: u64,
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

#[repr(C)]
#[derive(Copy, Clone, Debug)]
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
    pub fn get_name(&self) -> &str {
        let end = self.task_name.iter().position(|&c| c == 0).unwrap_or(32);
        core::str::from_utf8(&self.task_name[..end]).unwrap_or("invalid")
    }

    pub fn get_syscall_count(&self, syscall_id: usize) -> u32 {
        if syscall_id < self.syscall_stats.syscall_counts.len() {
            self.syscall_stats.syscall_counts[syscall_id] as u32
        } else {
            0
        }
    }

    pub fn get_total_calls(&self) -> u32 {
        self.syscall_stats.syscall_counts.iter().sum()
    }

    pub fn get_execution_time(&self) -> u64 {
        self.time_stats.execution_time
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
        syscall_stats: SyscallStats {
            syscall_counts: [0; 64],
        },
        exception_info: ExceptionInfo {
            exception_type: ExceptionType::None,
            fault_address: 0,
            instruction_address: 0,
            instruction_value: 0,
            exception_count: 0,
        },
        time_stats: TimeStats {
            start_time: 0,
            end_time: 0,
            execution_time: 0,
        },
        exit_code: 0,
        status: TaskStatus::NotStarted,
    };

    sys_get_taskinfo(&mut ti as *mut TaskInfo as *mut u8);

    ti
}

impl core::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TaskStatus::NotStarted => write!(f, "NotStarted"),
            TaskStatus::Running => write!(f, "Running"),
            TaskStatus::Completed => write!(f, "Completed"),
            TaskStatus::Exception => write!(f, "Exception"),
            TaskStatus::Exited => write!(f, "Exited"),
        }
    }
}

impl core::fmt::Display for ExceptionType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ExceptionType::None => write!(f, "None"),
            ExceptionType::InstructionMisaligned => write!(f, "InstructionMisaligned"),
            ExceptionType::InstructionFault => write!(f, "InstructionFault"),
            ExceptionType::IllegalInstruction => write!(f, "IllegalInstruction"),
            ExceptionType::Breakpoint => write!(f, "Breakpoint"),
            ExceptionType::LoadFault => write!(f, "LoadFault"),
            ExceptionType::StoreMisaligned => write!(f, "StoreMisaligned"),
            ExceptionType::StoreFault => write!(f, "StoreFault"),
            ExceptionType::UserEnvCall => write!(f, "UserEnvCall"),
            ExceptionType::VirtualSupervisorEnvCall => write!(f, "VirtualSupervisorEnvCall"),
            ExceptionType::InstructionPageFault => write!(f, "InstructionPageFault"),
            ExceptionType::LoadPageFault => write!(f, "LoadPageFault"),
            ExceptionType::StorePageFault => write!(f, "StorePageFault"),
            ExceptionType::InstructionGuestPageFault => write!(f, "InstructionGuestPageFault"),
            ExceptionType::LoadGuestPageFault => write!(f, "LoadGuestPageFault"),
            ExceptionType::VirtualInstruction => write!(f, "VirtualInstruction"),
            ExceptionType::StoreGuestPageFault => write!(f, "StoreGuestPageFault"),
            ExceptionType::Unknown => write!(f, "Unknown"),
        }
    }
}
