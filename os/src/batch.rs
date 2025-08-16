use crate::println;
use crate::sync::UPSafeCell;
use crate::task::{ExceptionType, TaskInfo};
use crate::trap::TrapContext;
use core::arch::asm;
use lazy_static::*;

const USER_STACK_SIZE: usize = 4096 * 2;
const KERNEL_STACK_SIZE: usize = 4096 * 2;
const MAX_APP_NUM: usize = 16;
const APP_BASE_ADDRESS: usize = 0x80400000;
const APP_SIZE_LIMIT: usize = 0x20000;

mod app_names {
    include!("app_names.rs");
}

#[repr(align(4096))]
struct KernelStack {
    data: [u8; KERNEL_STACK_SIZE],
}

#[repr(align(4096))]
struct UserStack {
    data: [u8; USER_STACK_SIZE],
}

static KERNEL_STACK: KernelStack = KernelStack {
    data: [0; KERNEL_STACK_SIZE],
};

static USER_STACK: UserStack = UserStack {
    data: [0; USER_STACK_SIZE],
};

impl KernelStack {
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + KERNEL_STACK_SIZE
    }

    pub fn push_context(&self, cx: TrapContext) -> &'static mut TrapContext {
        let cx_ptr = (self.get_sp() - core::mem::size_of::<TrapContext>()) as *mut TrapContext;
        unsafe {
            *cx_ptr = cx;
        }
        unsafe { cx_ptr.as_mut().unwrap() }
    }
}

impl UserStack {
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + USER_STACK_SIZE
    }
}

struct AppManager {
    num_app: usize,
    current_app: usize,
    app_start: [usize; MAX_APP_NUM + 1],
    app_task_info: [TaskInfo; MAX_APP_NUM],
}

impl AppManager {
    pub fn print_app_info(&self) {
        println!("[kernel] num_app = {}", self.num_app);
        for i in 0..self.num_app {
            println!(
                "[kernel] app_{} [{:#x}, {:#x})",
                i,
                self.app_start[i],
                self.app_start[i + 1]
            );
        }
    }

    fn load_app(&mut self, app_id: usize) {
        if app_id >= self.num_app {
            println!("All applications completed!");

            self.print_detailed_stats();

            #[cfg(feature = "board_qemu")]
            use crate::board::QEMUExit;
            #[cfg(feature = "board_qemu")]
            crate::board::QEMU_EXIT_HANDLE.exit_success();

            #[cfg(feature = "board_k210")]
            panic!("All applications completed!");
        }

        if self.current_app > 0 && self.current_app - 1 < self.app_task_info.len() {
            self.start_timing();
        }

        println!("[kernel] Loading app_{} ...", app_id);

        unsafe {
            asm!("fence.i");

            core::slice::from_raw_parts_mut(APP_BASE_ADDRESS as *mut u8, APP_SIZE_LIMIT).fill(0);

            let app_src = core::slice::from_raw_parts(
                self.app_start[app_id] as *const u8,
                self.app_start[app_id + 1] - self.app_start[app_id],
            );
            let app_dst =
                core::slice::from_raw_parts_mut(APP_BASE_ADDRESS as *mut u8, app_src.len());
            app_dst.copy_from_slice(app_src);
        }
    }

    pub fn get_current_app(&self) -> usize {
        self.current_app
    }

    pub fn move_to_next_app(&mut self) {
        self.current_app += 1;
    }

    pub fn get_current_task_info(&self) -> TaskInfo {
        let task_id = self.current_app;
        let task_name = if task_id > 0 && task_id - 1 < app_names::APP_NAMES.len() {
            app_names::APP_NAMES[task_id - 1]
        } else if task_id == 0 {
            "not_started"
        } else {
            "unknown"
        };

        let mut task_info = TaskInfo::new(task_id, task_name);

        if task_id > 0 && task_id - 1 < self.app_task_info.len() {
            task_info = self.app_task_info[task_id - 1].clone();
        }

        task_info
    }

    pub fn record_syscall(&mut self, syscall_id: usize) {
        let current_app = self.current_app;
        if current_app > 0 && current_app - 1 < self.app_task_info.len() {
            self.app_task_info[current_app - 1].record_syscall(syscall_id);
        }
    }

    pub fn record_exception(
        &mut self,
        exc_type: ExceptionType,
        fault_addr: usize,
        inst_addr: usize,
        inst_val: u32,
    ) {
        let current_app = self.current_app;
        if current_app > 0 && current_app - 1 < self.app_task_info.len() {
            self.app_task_info[current_app - 1]
                .record_exception(exc_type, fault_addr, inst_addr, inst_val);
        }
    }

    pub fn set_exit_code(&mut self, code: i32) {
        let current_app = self.current_app;
        if current_app > 0 && current_app - 1 < self.app_task_info.len() {
            self.app_task_info[current_app - 1].set_exit_code(code);
        }
    }

    pub fn start_timing(&mut self) {
        let current_app = self.current_app;
        if current_app > 0 && current_app - 1 < self.app_task_info.len() {
            let current_time = self.get_current_time();
            self.app_task_info[current_app - 1].start_timing(current_time);
        }
    }

    pub fn end_timing(&mut self) {
        let current_app = self.current_app;
        if current_app > 0 && current_app - 1 < self.app_task_info.len() {
            let current_time = self.get_current_time();
            self.app_task_info[current_app - 1].end_timing(current_time);
        }
    }

    pub fn get_current_time(&self) -> u64 {
        let mut time: u64;
        unsafe {
            asm!("csrr {}, time", out(reg) time);
        }
        time
    }

    pub fn print_detailed_stats(&self) {
        println!("=== Detailed Application Statistics ===");
        for i in 0..self.num_app {
            let app_name = if i < app_names::APP_NAMES.len() {
                app_names::APP_NAMES[i]
            } else {
                "unknown"
            };

            let task_info = &self.app_task_info[i];
            println!("App {} ({}):", i, app_name);
            println!("  Status: {:?}", task_info.status);
            println!("  Exit Code: {}", task_info.exit_code);

            // 时间统计
            if task_info.time_stats.execution_time > 0 {
                println!(
                    "  Execution Time: {} cycles",
                    task_info.time_stats.execution_time
                );
            }

            // 系统调用统计
            let total_syscalls = task_info.syscall_stats.get_total_calls();
            if total_syscalls > 0 {
                println!("  System Calls: {} total", total_syscalls);
                let syscall_names = [(64, "write"), (93, "exit"), (410, "get_taskinfo")];
                for (id, name) in &syscall_names {
                    let count = task_info.syscall_stats.get_count(*id);
                    if count > 0 {
                        println!("    {} ({}): {} times", name, id, count);
                    }
                }
            }

            // 异常统计
            if task_info.exception_info.exception_count > 0 {
                println!("  Exception: {:?}", task_info.exception_info.exception_type);
                println!(
                    "    Fault Address: 0x{:016x}",
                    task_info.exception_info.fault_address
                );
                println!(
                    "    Instruction Address: 0x{:016x}",
                    task_info.exception_info.instruction_address
                );
                println!(
                    "    Instruction Value: 0x{:08x}",
                    task_info.exception_info.instruction_value
                );
                println!("    Count: {}", task_info.exception_info.exception_count);
            }

            println!("");
        }
        println!("=== End of Statistics ===");
    }
}

lazy_static! {
    static ref APP_MANAGER: UPSafeCell<AppManager> = unsafe {
        UPSafeCell::new({
            unsafe extern "C" {
                fn _num_app();
            }
            let num_app_ptr = _num_app as usize as *const usize;
            let num_app = num_app_ptr.read_volatile();
            let mut app_start: [usize; MAX_APP_NUM + 1] = [0; MAX_APP_NUM + 1];
            let app_start_raw: &[usize] =
                core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1);
            app_start[..=num_app].copy_from_slice(app_start_raw);

            let mut app_task_info = [TaskInfo::new(0, ""); MAX_APP_NUM];
            for i in 0..num_app {
                let app_name = if i < app_names::APP_NAMES.len() {
                    app_names::APP_NAMES[i]
                } else {
                    "unknown"
                };
                app_task_info[i] = TaskInfo::new(i, app_name);
            }
            AppManager {
                num_app,
                current_app: 0,
                app_start,
                app_task_info,
            }
        })
    };
}

pub fn init() {
    print_app_info();
}

pub fn print_app_info() {
    APP_MANAGER.exclusive_access().print_app_info();
}

pub fn run_next_app() -> ! {
    let mut app_manager = APP_MANAGER.exclusive_access();
    let current_app = app_manager.get_current_app();
    unsafe {
        app_manager.load_app(current_app);
    }
    app_manager.move_to_next_app();
    drop(app_manager);

    unsafe extern "C" {
        fn __restore(cx_addr: usize);
    }
    unsafe {
        __restore(KERNEL_STACK.push_context(TrapContext::app_init_context(
            APP_BASE_ADDRESS,
            USER_STACK.get_sp(),
        )) as *const _ as usize);
    }
    panic!("Unreachable in batch::run_current_app!");
}

pub fn get_current_task_info() -> TaskInfo {
    APP_MANAGER.exclusive_access().get_current_task_info()
}

pub fn record_syscall(syscall_id: usize) {
    APP_MANAGER.exclusive_access().record_syscall(syscall_id);
}

pub fn record_exception(
    exc_type: ExceptionType,
    fault_addr: usize,
    inst_addr: usize,
    inst_val: u32,
) {
    APP_MANAGER
        .exclusive_access()
        .record_exception(exc_type, fault_addr, inst_addr, inst_val);
}

pub fn set_exit_code(code: i32) {
    APP_MANAGER.exclusive_access().set_exit_code(code);
}

pub fn end_timing() {
    APP_MANAGER.exclusive_access().end_timing();
}
