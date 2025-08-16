use crate::batch::set_exit_code;
use crate::batch::{end_timing, record_exception, run_next_app};
use crate::println;
use crate::stack_trace::print_stack_trace;
use crate::syscall::syscall;
use crate::task::ExceptionType;
use core::arch::global_asm;
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Trap},
    sepc, stval, stvec,
};

mod context;

global_asm!(include_str!("trap.S"));

pub fn init() {
    unsafe extern "C" {
        fn __alltraps();
    }
    unsafe {
        stvec::write(__alltraps as usize, TrapMode::Direct);
    }
}

#[unsafe(no_mangle)]
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    let scause = scause::read();
    let stval = stval::read();
    let sepc_val = sepc::read();

    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            cx.sepc += 4;
            cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
        }
        Trap::Exception(Exception::StoreFault) | Trap::Exception(Exception::StorePageFault) => {
            println!("[kernel] PageFault in application, kernel killed it.");

            let exc_type = if scause.cause() == Trap::Exception(Exception::StoreFault) {
                ExceptionType::StoreFault
            } else {
                ExceptionType::StorePageFault
            };

            let inst_val = unsafe { *(sepc_val as *const u32) };

            record_exception(exc_type, stval, sepc_val, inst_val);
            set_exit_code(-1);
            end_timing();

            unsafe { print_stack_trace() };

            run_next_app();
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction in application, kernel killed it.");

            let inst_val = unsafe { *(sepc_val as *const u32) };

            record_exception(ExceptionType::IllegalInstruction, stval, sepc_val, inst_val);
            set_exit_code(-1);
            end_timing();

            unsafe { print_stack_trace() };

            run_next_app();
        }
        _ => {
            println!("[kernel] Unsupported trap {:?}", scause.cause());
            println!("[kernel] Trap at 0x{:08x}", sepc_val);

            let exc_type = match scause.cause() {
                Trap::Exception(Exception::InstructionMisaligned) => {
                    ExceptionType::InstructionMisaligned
                }
                Trap::Exception(Exception::InstructionFault) => ExceptionType::InstructionFault,
                Trap::Exception(Exception::IllegalInstruction) => ExceptionType::IllegalInstruction,
                Trap::Exception(Exception::Breakpoint) => ExceptionType::Breakpoint,
                Trap::Exception(Exception::LoadFault) => ExceptionType::LoadFault,
                Trap::Exception(Exception::StoreMisaligned) => ExceptionType::StoreMisaligned,
                Trap::Exception(Exception::StoreFault) => ExceptionType::StoreFault,
                Trap::Exception(Exception::UserEnvCall) => ExceptionType::UserEnvCall,
                Trap::Exception(Exception::VirtualSupervisorEnvCall) => {
                    ExceptionType::VirtualSupervisorEnvCall
                }
                Trap::Exception(Exception::InstructionPageFault) => {
                    ExceptionType::InstructionPageFault
                }
                Trap::Exception(Exception::LoadPageFault) => ExceptionType::LoadPageFault,
                Trap::Exception(Exception::StorePageFault) => ExceptionType::StorePageFault,
                Trap::Exception(Exception::InstructionGuestPageFault) => {
                    ExceptionType::InstructionGuestPageFault
                }
                Trap::Exception(Exception::LoadGuestPageFault) => ExceptionType::LoadGuestPageFault,
                Trap::Exception(Exception::VirtualInstruction) => ExceptionType::VirtualInstruction,
                Trap::Exception(Exception::StoreGuestPageFault) => {
                    ExceptionType::StoreGuestPageFault
                }
                _ => ExceptionType::Unknown,
            };

            record_exception(exc_type, stval, sepc_val, 0);
            set_exit_code(-1);
            end_timing();

            unsafe { print_stack_trace() };

            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval,
            );
        }
    }
    cx
}

pub use context::TrapContext;
