#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;
use NimlothOS::serial_println;
use lazy_static::lazy_static;
use x86_64::structures::idt::InterruptDescriptorTable;
use NimlothOS::{exit_qemu, QemuExitCode};
use x86_64::structures::idt::InterruptStackFrame;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    serial_println!("stack overflow::stack_overflows...\t");

    NimlothOS::gdt::init();
    init_test_idt();

    stack_overflow();

    panic!("Execution continued after stack overflow");
}

#[allow(unconditional_recursion)]
fn stack_overflow() {
    stack_overflow();
    volatile::Volatile::new(0).read();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    NimlothOS::test_panic_handler(info)
}

// TODO: 恢复成正常代码
lazy_static! {
    static ref TEST_IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        unsafe {
            idt.double_fault.set_handler_fn(
                core::mem::transmute(test_double_fault_handler as *mut ())
            ).set_stack_index(NimlothOS::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt
    };
}

fn init_test_idt() {
    TEST_IDT.load();
}

// TODO: 恢复成正常代码
extern "x86-interrupt" fn test_double_fault_handler(
    _stack_frame: InterruptStackFrame, 
    _error_code: u64) {
    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    loop {}
}
