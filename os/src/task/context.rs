#[repr(C)]
#[derive(Copy, Clone)]
pub struct TaskContext {
    /// return address
    ra: usize,
    /// stack pointer
    sp: usize,
    /// callee saved registers
    s: [usize; 12],
}

impl TaskContext {
    pub fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }

    pub fn goto_restore(kstack_ptr: usize) -> Self {
        unsafe extern "C" {
            fn __pre_restore();
        }
        Self {
            ra: __pre_restore as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
    }
}
