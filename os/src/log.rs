use log::{self, Level, LevelFilter, Log, Metadata, Record};

use crate::println;

struct SimpleLogger;

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // -- release：只处理高于Info等级的
        // metadata.level() <= Level::Info
        // -- debug：全部
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let color = match record.level() {
                Level::Error => 31, // red
                Level::Warn => 93,  // bright yellow
                Level::Info => 34,  // blue
                Level::Debug => 32, // Green
                Level::Trace => 90, // bright black
            };
            let timestamp = get_timestamp();
            let cpu_id = get_cpu_id();
            let thread_id = get_thread_id();
            let module = record.target();

            if let (Some(file), Some(line)) = (record.file(), record.line()) {
                let file_name = file.split('/').last().unwrap_or(file);
                println!(
                    "\u{1B}[{}m{:>5} [T{:>4}] [CPU{}] [TH{}] [{}] [{}:{}] {}\u{1B}[0m",
                    color,
                    record.level(),
                    timestamp,
                    cpu_id,
                    thread_id,
                    module,
                    file_name,
                    line,
                    record.args()
                );
            } else {
                println!(
                    "\u{1B}[{}m{:>5} [T{:>4}] [CPU{}] [TH{}] [{}] [unknown] {}\u{1B}[0m",
                    color,
                    record.level(),
                    timestamp,
                    cpu_id,
                    thread_id,
                    module,
                    record.args()
                );
            }
        }
        return;
    }

    fn flush(&self) {}
}

static mut TICK_COUNT: usize = 0;

fn get_timestamp() -> usize {
    unsafe {
        TICK_COUNT += 1;
        TICK_COUNT
    }
}

fn get_cpu_id() -> usize {
    unsafe {
        let cpu_id: usize;
        core::arch::asm!("csrr {}, mhartid", out(reg) cpu_id, options(nomem, nostack));
        cpu_id
    }
}

fn get_thread_id() -> usize {
    unsafe {
        let thread_id: usize;
        core::arch::asm!("csrr {}, mhartid", out(reg) thread_id, options(nomem, nostack));
        thread_id
    }
}

pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;

    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(match option_env!("LOG") {
        Some("ERROR") => LevelFilter::Error,
        Some("WARN") => LevelFilter::Warn,
        Some("INFO") => LevelFilter::Info,
        Some("DEBUG") => LevelFilter::Debug,
        Some("TRACE") => LevelFilter::Trace,
        _ => LevelFilter::Info,
    });
}

/*
#[macro_export]
macro_rules! error {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[31mERROR {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

#[macro_export]
macro_rules! warn {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[93m WARN {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

#[macro_export]
macro_rules! info {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[34m INFO {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

#[macro_export]
macro_rules! debug {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[32mDEBUG {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}

#[macro_export]
macro_rules! trace {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::println!("\u{1B}[90mTRACE {}\u{1B}[0m", format_args!($fmt $(, $($arg)+)?))
    };
}
*/
