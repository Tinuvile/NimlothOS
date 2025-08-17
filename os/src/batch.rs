use crate::config::*;
use crate::println;
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use core::arch::asm;
use core::str;
use lazy_static::*;

#[repr(align(4096))]
#[derive(Clone, Copy)]
struct KernelStack {
    data: [u8; KERNEL_STACK_SIZE],
}

#[repr(align(4096))]
#[derive(Clone, Copy)]
struct UserStack {
    data: [u8; USER_STACK_SIZE],
}

static KERNEL_STACK: [KernelStack; MAX_APP_NUM] = [KernelStack {
    data: [0; KERNEL_STACK_SIZE],
}; MAX_APP_NUM];

static USER_STACK: [UserStack; MAX_APP_NUM] = [UserStack {
    data: [0; USER_STACK_SIZE],
}; MAX_APP_NUM];

impl KernelStack {
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + KERNEL_STACK_SIZE
    }

    pub fn push_context(&self, trap_cx: TrapContext) -> &'static mut TrapContext {
        let trap_cx_ptr = (self.get_sp() - core::mem::size_of::<TrapContext>()) as *mut TrapContext;
        unsafe {
            *trap_cx_ptr = trap_cx;
        }
        unsafe { trap_cx_ptr.as_mut().unwrap() }
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

    pub fn get_current_app(&self) -> usize {
        self.current_app
    }

    pub fn move_to_next_app(&mut self) {
        self.current_app += 1;
    }

    pub fn get_current_app_base(&self) -> usize {
        APP_BASE_ADDRESS + self.current_app * APP_SIZE_LIMIT
    }

    pub fn get_current_app_range(&self) -> (usize, usize) {
        let running_app_id = if self.current_app > 0 {
            self.current_app - 1
        } else {
            return (0, 0);
        };

        if running_app_id >= self.num_app {
            return (0, 0);
        }

        let app_base = APP_BASE_ADDRESS + running_app_id * APP_SIZE_LIMIT;
        let app_size = self.app_start[running_app_id + 1] - self.app_start[running_app_id];
        (app_base, app_base + app_size)
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
            AppManager {
                num_app,
                current_app: 0,
                app_start,
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

    if current_app >= app_manager.num_app {
        println!("[kernel] All applications completed!");

        #[cfg(feature = "board_qemu")]
        use crate::board::QEMUExit;
        #[cfg(feature = "board_qemu")]
        crate::board::QEMU_EXIT_HANDLE.exit_success();

        #[cfg(feature = "board_k210")]
        panic!("All applications completed!");
    }

    println!("[kernel] Loading app_{} ...", current_app);

    let app_base = app_manager.get_current_app_base();

    app_manager.move_to_next_app();
    drop(app_manager);

    unsafe extern "C" {
        fn __restore(cx_addr: usize);
    }
    unsafe {
        __restore(
            KERNEL_STACK[current_app].push_context(TrapContext::app_init_context(
                app_base,
                USER_STACK[current_app].get_sp(),
            )) as *const _ as usize,
        );
    }
    panic!("Unreachable in batch::run_next_app!");
}

pub fn get_current_app_range() -> (usize, usize) {
    APP_MANAGER.exclusive_access().get_current_app_range()
}

pub fn get_user_stack_range() -> (usize, usize) {
    let app_manager = APP_MANAGER.exclusive_access();
    let current_app = if app_manager.current_app > 0 {
        app_manager.current_app - 1
    } else {
        0
    };
    drop(app_manager);

    if current_app < MAX_APP_NUM {
        let stack_top = USER_STACK[current_app].get_sp();
        (stack_top - USER_STACK_SIZE, stack_top)
    } else {
        (0, 0)
    }
}
