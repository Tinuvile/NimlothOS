use crate::{println, sbi::shutdown};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "Paniced at {}:{}:{}",
            location.file(),
            location.line(),
            info.message()
        );
    } else {
        println!("Paniced: {}", info.message());
    }
    shutdown();
}
