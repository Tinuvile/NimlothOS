use crate::loader::{get_num_app, init_app_cx};
use crate::println;
use crate::sbi::shutdown;
use crate::task::switch::__switch;
use crate::timer::get_time_ms;
use crate::{
    config::{MAX_APP_NUM, MAX_SYSCALL_COUNT},
    sync::UPSafeCell,
};
use lazy_static::*;
use task::TaskControlBlock;

pub use context::TaskContext;
pub use task::{SyscallInfo, TaskInfo, TaskStatus};

mod context;
mod switch;

#[allow(clippy::module_inception)]
mod task;

pub struct TaskManager {
    num_app: usize,
    inner: UPSafeCell<TaskManagerInner>,
}

pub struct TaskManagerInner {
    tasks: [TaskControlBlock; MAX_APP_NUM],
    current_task: usize,
    stop_watch: usize,
}

impl TaskManagerInner {
    fn refresh_stop_watch(&mut self) -> usize {
        let start_time = self.stop_watch;
        self.stop_watch = get_time_ms();
        self.stop_watch - start_time
    }
}

lazy_static! {
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let mut tasks = [TaskControlBlock {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::Uninit,
            user_time: 0,
            kernel_time: 0,
            syscall_times: [0; MAX_SYSCALL_COUNT],
        }; MAX_APP_NUM];
        for (i, task) in tasks.iter_mut().enumerate() {
            task.task_cx = TaskContext::goto_restore(init_app_cx(i));
            task.task_status = TaskStatus::Ready;
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                    stop_watch: 0,
                })
            },
        }
    };
}

impl TaskManager {
    fn run_first_task(&self) {
        let mut inner = self.inner.exclusive_access();
        let task0 = &mut inner.tasks[0];
        task0.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &task0.task_cx as *const TaskContext;
        inner.refresh_stop_watch();
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        unsafe {
            __switch(&mut _unused as *mut TaskContext, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;
        inner.tasks[current_task].kernel_time += inner.refresh_stop_watch();
        inner.tasks[current_task].task_status = TaskStatus::Ready;
    }

    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].kernel_time += inner.refresh_stop_watch();
        println!(
            "[task {}] exited, user_time: {} ms, kernel_time: {} ms",
            current, inner.tasks[current].user_time, inner.tasks[current].kernel_time
        );
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;

        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            println!("All applications completed!");
            shutdown();
        }
    }

    fn user_time_start(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].user_time += inner.refresh_stop_watch();
    }

    fn user_time_end(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].user_time += inner.refresh_stop_watch();
    }

    fn get_current_task_id(&self) -> usize {
        let mut inner = self.inner.exclusive_access();
        inner.current_task
    }

    fn get_current_task_status(&self) -> TaskStatus {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status
    }

    fn record_syscall_times(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;

        let index = match syscall_id {
            64 => 0,
            93 => 1,
            124 => 2,
            169 => 3,
            410 => 4,
            _ => return,
        };

        if index < MAX_SYSCALL_COUNT {
            inner.tasks[current].syscall_times[index] += 1;
        }
    }

    fn get_current_task_syscall_times(&self) -> [SyscallInfo; MAX_SYSCALL_COUNT] {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let syscall_times = inner.tasks[current].syscall_times;
        let mut syscall_infos = [SyscallInfo { id: 0, times: 0 }; MAX_SYSCALL_COUNT];
        for (i, times) in syscall_times.iter().enumerate() {
            syscall_infos[i] = SyscallInfo {
                id: i,
                times: *times,
            };
        }
        syscall_infos
    }

    fn get_current_task_time(&self) -> usize {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].user_time + inner.tasks[current].kernel_time
    }
}

pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

pub fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

pub fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

pub fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

pub fn user_time_start() {
    TASK_MANAGER.user_time_start();
}

pub fn user_time_end() {
    TASK_MANAGER.user_time_end();
}

pub fn get_current_task_id() -> usize {
    TASK_MANAGER.get_current_task_id()
}

pub fn get_current_task_status() -> TaskStatus {
    TASK_MANAGER.get_current_task_status()
}

pub fn get_current_task_syscall_times() -> [SyscallInfo; MAX_SYSCALL_COUNT] {
    TASK_MANAGER.get_current_task_syscall_times()
}

pub fn get_current_task_time() -> usize {
    TASK_MANAGER.get_current_task_time()
}

pub fn record_syscall_times(syscall_id: usize) {
    TASK_MANAGER.record_syscall_times(syscall_id);
}
