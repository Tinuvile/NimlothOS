use crate::println;
use bitflags::*;

pub const MAX_SIG: usize = 31;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct SignalFlags: u32 {
        const SIGDEF = 1;
        const SIGHUP = 1 << 1;
        const SIGINT = 1 << 2;
        const SIGQUIT = 1 << 3;
        const SIGILL = 1 << 4;
        const SIGTRAP = 1 << 5;
        const SIGABRT = 1 << 6;
        const SIGBUS = 1 << 7;
        const SIGFPE = 1 << 8;
        const SIGKILL = 1 << 9;
        const SIGUSR1 = 1 << 10;
        const SIGSEGV = 1 << 11;
        const SIGUSR2 = 1 << 12;
        const SIGPIPE = 1 << 13;
        const SIGALRM = 1 << 14;
        const SIGTERM = 1 << 15;
        const SIGSTKFLT = 1 << 16;
        const SIGCHLD = 1 << 17;
        const SIGCONT = 1 << 18;
        const SIGSTOP = 1 << 19;
        const SIGTSTP = 1 << 20;
        const SIGTTIN = 1 << 21;
        const SIGTTOU = 1 << 22;
        const SIGURG = 1 << 23;
        const SIGXCPU = 1 << 24;
        const SIGXFSZ = 1 << 25;
        const SIGVTALRM = 1 << 26;
        const SIGPROF = 1 << 27;
        const SIGWINCH = 1 << 28;
        const SIGIO = 1 << 29;
        const SIGPWR = 1 << 30;
        const SIGSYS = 1 << 31;
    }
}

impl SignalFlags {
    pub fn check_error(&self) -> Option<(i32, &'static str)> {
        if self.contains(Self::SIGHUP) {
            Some((-1, "Hangup, SIGHUP=1"))
        } else if self.contains(Self::SIGINT) {
            Some((-2, "Killed, SIGINT=2"))
        } else if self.contains(Self::SIGQUIT) {
            Some((-3, "Killed, SIGQUIT=3"))
        } else if self.contains(Self::SIGILL) {
            Some((-4, "Illegal Instruction, SIGILL=4"))
        } else if self.contains(Self::SIGTRAP) {
            Some((-5, "Trace/breakpoint trap, SIGTRAP=5"))
        } else if self.contains(Self::SIGABRT) {
            Some((-6, "Aborted, SIGABRT=6"))
        } else if self.contains(Self::SIGBUS) {
            Some((-7, "Bus error, SIGBUS=7"))
        } else if self.contains(Self::SIGFPE) {
            Some((-8, "Erroneous Arithmetic Operation, SIGFPE=8"))
        } else if self.contains(Self::SIGKILL) {
            Some((-9, "Killed, SIGKILL=9"))
        } else if self.contains(Self::SIGUSR1) {
            Some((-10, "Killed, SIGUSR1=10"))
        } else if self.contains(Self::SIGSEGV) {
            Some((-11, "Segmentation Fault, SIGSEGV=11"))
        } else if self.contains(Self::SIGUSR2) {
            Some((-12, "Killed, SIGUSR2=12"))
        } else if self.contains(Self::SIGPIPE) {
            Some((-13, "Broken pipe, SIGPIPE=13"))
        } else if self.contains(Self::SIGALRM) {
            Some((-14, "Killed, SIGALRM=14"))
        } else if self.contains(Self::SIGTERM) {
            Some((-15, "Killed, SIGTERM=15"))
        } else if self.contains(Self::SIGSTKFLT) {
            Some((-16, "Stack fault, SIGSTKFLT=16"))
        } else if self.contains(Self::SIGCHLD) {
            Some((-17, "Child terminated, SIGCHLD=17"))
        } else if self.contains(Self::SIGCONT) {
            Some((-18, "Continued, SIGCONT=18"))
        } else if self.contains(Self::SIGSTOP) {
            Some((-19, "Stopped, SIGSTOP=19"))
        } else if self.contains(Self::SIGTSTP) {
            Some((-20, "Stopped, SIGTSTP=20"))
        } else if self.contains(Self::SIGTTIN) {
            Some((-21, "Stopped, SIGTTIN=21"))
        } else if self.contains(Self::SIGTTOU) {
            Some((-22, "Stopped, SIGTTOU=22"))
        } else if self.contains(Self::SIGURG) {
            Some((-23, "Urgent condition, SIGURG=23"))
        } else if self.contains(Self::SIGXCPU) {
            Some((-24, "CPU time limit exceeded, SIGXCPU=24"))
        } else if self.contains(Self::SIGXFSZ) {
            Some((-25, "File size limit exceeded, SIGXFSZ=25"))
        } else if self.contains(Self::SIGVTALRM) {
            Some((-26, "Virtual time alarm, SIGVTALRM=26"))
        } else if self.contains(Self::SIGPROF) {
            Some((-27, "Profiling time alarm, SIGPROF=27"))
        } else if self.contains(Self::SIGWINCH) {
            Some((-28, "Window size changed, SIGWINCH=28"))
        } else if self.contains(Self::SIGIO) {
            Some((-29, "I/O possible, SIGIO=29"))
        } else if self.contains(Self::SIGPWR) {
            Some((-30, "Power failure, SIGPWR=30"))
        } else if self.contains(Self::SIGSYS) {
            Some((-31, "Bad system call, SIGSYS=31"))
        } else {
            // println!("[K] signalflags check_error  {:?}", self);
            None
        }
    }
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalAction {
    pub handler: usize,
    pub mask: SignalFlags,
}

impl Default for SignalAction {
    fn default() -> Self {
        Self {
            handler: 0,
            mask: SignalFlags::from_bits(40).unwrap(),
        }
    }
}

#[derive(Clone)]
pub struct SignalActions {
    pub table: [SignalAction; MAX_SIG + 1],
}

impl Default for SignalActions {
    fn default() -> Self {
        Self {
            table: [SignalAction::default(); MAX_SIG + 1],
        }
    }
}
