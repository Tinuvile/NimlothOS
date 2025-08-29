#![no_std]
#![no_main]
#![allow(clippy::println_empty_string)]

extern crate alloc;

#[macro_use]
extern crate user_lib;

const LF: u8 = 0x0au8;
const CR: u8 = 0x0du8;
const DL: u8 = 0x7fu8;
const BS: u8 = 0x08u8;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use user_lib::console::getchar;
use user_lib::{
    OpenFlags, close, dup, exec, exit, fork, open, pid, pipe, read, time, waitpid, write, yield_,
};

// ANSI 颜色常量
const C_RESET: &str = "\x1b[0m";
const C_BOLD: &str = "\x1b[1m";
const C_RED: &str = "\x1b[31m";
const C_GREEN: &str = "\x1b[32m";
const C_YELLOW: &str = "\x1b[33m";
const C_BLUE: &str = "\x1b[34m";
const C_CYAN: &str = "\x1b[36m";
const C_MAGENTA: &str = "\x1b[35m";

/// 为文本添加颜色
#[inline]
fn colored(text: &str, color: &str) -> String {
    let mut output = String::new();
    output.push_str(color);
    output.push_str(text);
    output.push_str(C_RESET);
    output
}

/// 打印彩色提示符
#[inline]
fn print_prompt() {
    print!("{}", colored(">> ", &format!("{}{}", C_BOLD, C_CYAN)));
}

/// 打印错误信息到标准错误（红色）
#[inline]
fn eprintln_error(msg: &str) {
    let error_msg = colored(&format!("Error: {}", msg), C_RED);
    let _ = write(2, error_msg.as_bytes());
    let _ = write(2, b"\n");
}

/// 打印信息（青色）
#[inline]
fn println_info(msg: &str) {
    println!("{}", colored(msg, C_CYAN));
}

/// 打印成功信息（绿色）
#[inline]
fn println_success(msg: &str) {
    println!("{}", colored(msg, C_GREEN));
}

/// 打印警告信息（黄色）
#[inline]
fn println_warning(msg: &str) {
    println!("{}", colored(msg, C_YELLOW));
}

/// 执行内置命令
/// 返回 true 表示命令被处理，false 表示不是内置命令
fn execute_builtin_command(args: &[String]) -> bool {
    if args.is_empty() {
        return false;
    }

    let cmd = args[0].trim_end_matches('\0');

    match cmd {
        "help" => {
            builtin_help();
            true
        }
        "exit" => {
            builtin_exit(args);
            true
        }
        "pwd" => {
            builtin_pwd();
            true
        }
        "echo" => {
            builtin_echo(args);
            true
        }
        "clear" => {
            builtin_clear();
            true
        }
        "history" => {
            builtin_history();
            true
        }
        "ps" => {
            builtin_ps();
            true
        }
        "time" => {
            builtin_time();
            true
        }
        "sleep" => {
            builtin_sleep(args);
            true
        }
        "test" => {
            builtin_test(args);
            true
        }
        "version" => {
            builtin_version();
            true
        }
        "ls" => {
            builtin_ls();
            true
        }
        "programs" => {
            builtin_programs();
            true
        }
        _ => false,
    }
}

/// help 命令 - 显示帮助信息
fn builtin_help() {
    println!(
        "\n{}",
        colored(
            "=== NimlothOS Shell Help ===",
            &format!("{}{}", C_BOLD, C_CYAN)
        )
    );
    println!(
        "\n{}:",
        colored("Built-in Commands", &format!("{}", C_BOLD))
    );
    println!(
        "  {}      - Show this help message",
        colored("help", C_GREEN)
    );
    println!(
        "  {}  - Exit shell with optional code",
        colored("exit [code]", C_GREEN)
    );
    println!(
        "  {}       - Show current directory",
        colored("pwd", C_GREEN)
    );
    println!(
        "  {} - Print text to screen",
        colored("echo <text>", C_GREEN)
    );
    println!("  {}     - Clear the screen", colored("clear", C_GREEN));
    println!("  {}   - Show command history", colored("history", C_GREEN));
    println!("  {}        - Show process info", colored("ps", C_GREEN));
    println!("  {}      - Show system uptime", colored("time", C_GREEN));
    println!(
        "  {} - Sleep for milliseconds",
        colored("sleep <ms>", C_GREEN)
    );
    println!("  {} - Run system tests", colored("test <args>", C_GREEN));
    println!("  {}   - Show version info", colored("version", C_GREEN));
    println!(
        "  {}        - List directory contents",
        colored("ls", C_GREEN)
    );
    println!(
        "  {}   - List available programs",
        colored("programs", C_GREEN)
    );

    println!(
        "\n{}:",
        colored("External Programs", &format!("{}", C_BOLD))
    );
    println!(
        "  {}   - Display file contents",
        colored("cat <file>", C_YELLOW)
    );
    println!(
        "  {}  - Large file write test",
        colored("huge_write", C_YELLOW)
    );
    println!(
        "  {}      - Colorful text demo",
        colored("fantastic_text", C_YELLOW)
    );
    println!(
        "  {} - Test file operations",
        colored("filetest_simple", C_YELLOW)
    );
    println!(
        "  {}   - Test pipe operations",
        colored("pipetest", C_YELLOW)
    );
    println!(
        "  {}      - Test process operations",
        colored("forktest", C_YELLOW)
    );

    println!(
        "\n{}:",
        colored("Pipes & Redirection", &format!("{}", C_BOLD))
    );
    println!(
        "  {}      - Pipe operations",
        colored("cmd1 | cmd2", C_BLUE)
    );
    println!(
        "  {}    - Output redirection",
        colored("cmd > file", C_BLUE)
    );
    println!("  {}    - Input redirection", colored("cmd < file", C_BLUE));

    println!("\n{}:", colored("Hotkeys", &format!("{}", C_BOLD)));
    println!(
        "  {}    - Delete character",
        colored("Backspace", C_MAGENTA)
    );
    println!("  {}       - Execute command", colored("Enter", C_MAGENTA));
    print!("  ");
    print!("{}", colored("Ctrl+A, X", C_MAGENTA));
    println!("  - Exit QEMU");
    println!("");
}

/// exit 命令 - 退出 shell
fn builtin_exit(args: &[String]) {
    let code = if args.len() > 1 {
        args[1].trim_end_matches('\0').parse::<i32>().unwrap_or(0)
    } else {
        0
    };

    println_success(&format!("Goodbye! Exiting with code {}", code));
    exit(code);
}

/// pwd 命令 - 显示当前工作目录
fn builtin_pwd() {
    // 由于我们的文件系统比较简单，暂时只显示根目录
    println!("/");
}

/// echo 命令 - 输出文本
fn builtin_echo(args: &[String]) {
    if args.len() > 1 {
        let text: Vec<&str> = args[1..].iter().map(|s| s.trim_end_matches('\0')).collect();
        println!("{}", text.join(" "));
    } else {
        println!("");
    }
}

/// clear 命令 - 清屏
fn builtin_clear() {
    // ANSI 清屏序列
    print!("\x1b[2J\x1b[H");

    // 重新显示 logo
    let logo = r#"
 /$$   /$$ /$$               /$$             /$$     /$$        /$$$$$$   /$$$$$$ 
| $$$ | $$|__/              | $$            | $$    | $$       /$$__  $$ /$$__  $$
| $$$$| $$ /$$ /$$$$$$/$$$$ | $$  /$$$$$$  /$$$$$$  | $$$$$$$ | $$  \ $$| $$  \__/
| $$ $$ $$| $$| $$_  $$_  $$| $$ /$$__  $$|_  $$_/  | $$__  $$| $$  | $$|  $$$$$$ 
| $$  $$$$| $$| $$ \ $$ \ $$| $$| $$  \ $$  | $$    | $$  \ $$| $$  | $$ \____  $$
| $$\  $$$| $$| $$ | $$ | $$| $$| $$  | $$  | $$ /$$| $$  | $$| $$  | $$ /$$  \ $$
| $$ \  $$| $$| $$ | $$ | $$| $$|  $$$$$$/  |  $$$$/| $$  | $$|  $$$$$$/|  $$$$$$/
|__/  \__/|__/|__/ |__/ |__/|__/ \______/    \___/  |__/  |__/ \______/  \______/ 
"#;
    println!("{}", colored(logo, &format!("{}{}", C_BOLD, C_MAGENTA)));
    println_info("Terminal cleared. Welcome back!");
}

/// history 命令 - 显示命令历史
fn builtin_history() {
    println_warning("Command history is not implemented yet.");
    println!("This feature would require persistent storage.");
}

/// ps 命令 - 显示进程信息
fn builtin_ps() {
    println!(
        "{}",
        colored("Process Information:", &format!("{}", C_BOLD))
    );
    println!("  PID: {}", pid());
    println!("  Name: user_shell");
    println!("  Status: Running");
    println!("\nNote: Full process list requires kernel support.");
}

/// time 命令 - 显示当前时间
fn builtin_time() {
    let current_time = time();
    println!("System uptime: {} ms", current_time);

    // 计算简单的时间格式
    let seconds = current_time / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    if hours > 0 {
        println!("Formatted: {}h {}m {}s", hours, minutes % 60, seconds % 60);
    } else if minutes > 0 {
        println!("Formatted: {}m {}s", minutes, seconds % 60);
    } else {
        println!("Formatted: {}s", seconds);
    }
}

/// sleep 命令 - 睡眠指定时间
fn builtin_sleep(args: &[String]) {
    if args.len() < 2 {
        eprintln_error("Usage: sleep <milliseconds>");
        return;
    }

    let ms_str = args[1].trim_end_matches('\0');
    match ms_str.parse::<usize>() {
        Ok(ms) => {
            if ms > 10000 {
                println_warning("Sleeping for more than 10 seconds might be too long!");
            }
            println!("Sleeping for {} ms...", ms);

            // 简单的睡眠实现 - 通过 yield 循环
            let start_time = time();
            while time() - start_time < ms as isize {
                yield_();
            }

            println_success("Wake up!");
        }
        Err(_) => {
            eprintln_error("Invalid time format. Please enter a number.");
        }
    }
}

/// test 命令 - 测试功能
fn builtin_test(args: &[String]) {
    if args.len() < 2 {
        println!("{}", colored("Available tests:", &format!("{}", C_BOLD)));
        println!("  {} - Test file operations", colored("test file", C_GREEN));
        println!(
            "  {} - Test process operations",
            colored("test proc", C_GREEN)
        );
        println!("  {} - Test pipe operations", colored("test pipe", C_GREEN));
        println!("  {} - Show system info", colored("test system", C_GREEN));
        return;
    }

    let test_type = args[1].trim_end_matches('\0');
    match test_type {
        "file" => test_file_operations(),
        "proc" => test_process_operations(),
        "pipe" => test_pipe_operations(),
        "system" => test_system_info(),
        _ => {
            eprintln_error(&format!("Unknown test type: {}", test_type));
            println!("Run 'test' without arguments to see available tests.");
        }
    }
}

/// version 命令 - 显示版本信息
fn builtin_version() {
    println!(
        "{}",
        colored("NimlothOS Shell v1.2", &format!("{}{}", C_BOLD, C_CYAN))
    );
    println!("Built with: Rust (no_std)");
    println!("Features: Pipeline, Redirection, Built-in Commands");
    println!("Architecture: RISC-V");
    println!("License: Educational Use");
}

/// 测试文件操作
fn test_file_operations() {
    println!(
        "{}",
        colored("Testing file operations...", &format!("{}", C_BOLD))
    );

    // 创建测试文件
    let test_content = b"Hello from NimlothOS shell test!";
    let fd = open("shell_test.txt\0", OpenFlags::CREATE | OpenFlags::WRONLY);

    if fd >= 0 {
        let fd = fd as usize;
        let written = write(fd, test_content);
        close(fd);

        if written > 0 {
            println_success("✓ File write test passed");

            // 读取测试
            let fd = open("shell_test.txt\0", OpenFlags::RDONLY);
            if fd >= 0 {
                let fd = fd as usize;
                let mut buffer = [0u8; 100];
                let read_bytes = read(fd, &mut buffer);
                close(fd);

                if read_bytes > 0 {
                    println_success("✓ File read test passed");
                    if let Ok(content) = core::str::from_utf8(&buffer[..read_bytes as usize]) {
                        println!("  Content: {}", content);
                    } else {
                        println!("  Content: [Invalid UTF-8]");
                    }
                } else {
                    eprintln_error("✗ File read test failed");
                }
            } else {
                eprintln_error("✗ Cannot open file for reading");
            }
        } else {
            eprintln_error("✗ File write test failed");
        }
    } else {
        eprintln_error("✗ Cannot create test file");
    }
}

/// 测试进程操作
fn test_process_operations() {
    println!(
        "{}",
        colored("Testing process operations...", &format!("{}", C_BOLD))
    );

    println!("Current PID: {}", pid());

    let child_pid = fork();
    if child_pid == 0 {
        // 子进程
        println_success("✓ Child process created successfully");
        println!("  Child PID: {}", pid());
        exit(42);
    } else if child_pid > 0 {
        // 父进程
        println!("  Parent PID: {}, Child PID: {}", pid(), child_pid);

        let mut exit_code = 0;
        let result_pid = waitpid(child_pid as usize, &mut exit_code);

        if result_pid == child_pid {
            println_success("✓ Process wait test passed");
            println!("  Child exit code: {}", exit_code);
        } else {
            eprintln_error("✗ Process wait test failed");
        }
    } else {
        eprintln_error("✗ Fork failed");
    }
}

/// 测试管道操作
fn test_pipe_operations() {
    println!(
        "{}",
        colored("Testing pipe operations...", &format!("{}", C_BOLD))
    );

    let mut pipe_fd = [0usize; 2];
    if pipe(&mut pipe_fd) == 0 {
        println_success("✓ Pipe creation test passed");

        let test_data = b"Pipe test data";
        let child_pid = fork();

        if child_pid == 0 {
            // 子进程 - 写入数据
            close(pipe_fd[0]); // 关闭读端
            let written = write(pipe_fd[1], test_data);
            close(pipe_fd[1]);

            if written > 0 {
                exit(0); // 成功
            } else {
                exit(1); // 失败
            }
        } else if child_pid > 0 {
            // 父进程 - 读取数据
            close(pipe_fd[1]); // 关闭写端

            let mut buffer = [0u8; 50];
            let read_bytes = read(pipe_fd[0], &mut buffer);
            close(pipe_fd[0]);

            let mut exit_code = 0;
            waitpid(child_pid as usize, &mut exit_code);

            if read_bytes > 0 && exit_code == 0 {
                println_success("✓ Pipe communication test passed");
                if let Ok(content) = core::str::from_utf8(&buffer[..read_bytes as usize]) {
                    println!("  Received: {}", content);
                } else {
                    println!("  Received: [Invalid UTF-8]");
                }
            } else {
                eprintln_error("✗ Pipe communication test failed");
            }
        } else {
            eprintln_error("✗ Fork for pipe test failed");
        }
    } else {
        eprintln_error("✗ Pipe creation test failed");
    }
}

/// 显示系统信息
fn test_system_info() {
    println!("{}", colored("System Information:", &format!("{}", C_BOLD)));
    println!("  OS: NimlothOS");
    println!("  Architecture: RISC-V");
    println!("  Shell: Enhanced User Shell");
    println!("  Current Time: {} ms", time());
    println!("  Process ID: {}", pid());
    println!("  Features:");
    println!("    ✓ Multi-process support");
    println!("    ✓ File system operations");
    println!("    ✓ Pipe communication");
    println!("    ✓ Signal handling");
    println!("    ✓ Memory management");
    println!("    ✓ MLFQ scheduling");
}

/// ls 命令 - 列出目录内容
fn builtin_ls() {
    println!("{}", colored("Directory Contents:", &format!("{}", C_BOLD)));

    // 由于我们的文件系统比较简单，这里模拟显示根目录的内容
    // 实际的 ls 实现需要文件系统的目录遍历支持

    println!("\n{}:", colored("Files in current directory (/)", C_CYAN));

    // 检查是否存在通过测试创建的文件
    let test_files = ["filea", "testf", "shell_test.txt"];
    let mut found_files = false;

    for file in &test_files {
        // 尝试打开文件来检查是否存在
        let fd = open(&format!("{}\0", file), OpenFlags::RDONLY);
        if fd >= 0 {
            close(fd as usize);
            println!(
                "  {} {}",
                colored("-rw-r--r--", C_BLUE),
                colored(file, C_YELLOW)
            );
            found_files = true;
        }
    }

    if !found_files {
        println!("  {}", colored("(no user files found)", C_MAGENTA));
        println!(
            "  {}",
            colored("Run 'filetest_simple' to create test files", C_CYAN)
        );
    }

    println!("\n{}:", colored("System Info", C_GREEN));
    println!("  Current directory: {}", colored("/", C_CYAN));
    println!("  File system: MicroFS");
    println!("  Available space: ~16MB");

    println!("\n{}:", colored("Note", C_YELLOW));
    println!("  This is a simple file system demonstration.");
    println!("  For full directory listing, a more complex FS driver is needed.");
    println!(
        "  Use '{}' to see available programs.",
        colored("programs", C_GREEN)
    );
}

/// programs 命令 - 列出可用的程序
fn builtin_programs() {
    println!("{}", colored("Available Programs:", &format!("{}", C_BOLD)));

    // 显示内置命令
    println!("\n{}:", colored("Built-in Commands", C_GREEN));
    let builtins = [
        "help", "exit", "pwd", "echo", "clear", "history", "ps", "time", "sleep", "test",
        "version", "ls", "programs",
    ];

    for (i, cmd) in builtins.iter().enumerate() {
        if i % 4 == 0 && i != 0 {
            println!("");
        }
        print!("  {:12}", colored(cmd, C_GREEN));
    }
    println!("");

    // 显示外部程序
    println!("\n{}:", colored("External Programs", C_YELLOW));
    let programs = [
        "cat",
        "filetest_simple",
        "pipetest",
        "forktest",
        "hello_world",
        "fantastic_text",
        "getchar",
        "huge_write",
        "cmdline_args",
        "exit",
        "yield",
        "sleep_simple",
        "forktest2",
        "forktest_simple",
        "forktree",
        "matrix",
        "pipe_large_test",
        "run_pipe_test",
        "count_lines",
        "sig_simple",
        "sig_simple2",
        "sig_tests",
        "usertests",
    ];

    for (i, prog) in programs.iter().enumerate() {
        if i % 4 == 0 && i != 0 {
            println!("");
        }
        print!("  {:15}", colored(prog, C_YELLOW));
    }
    println!("");

    // 显示测试程序
    println!("\n{}:", colored("Test Programs", C_BLUE));
    let tests = [
        "priority_test",
        "io_priority_test",
        "mlfq_test",
        "mlfq_demo",
        "stack_overflow",
        "priv_csr",
        "priv_inst",
        "store_fault",
    ];

    for (i, test) in tests.iter().enumerate() {
        if i % 3 == 0 && i != 0 {
            println!("");
        }
        print!("  {:16}", colored(test, C_BLUE));
    }
    println!("");

    println!("\n{}:", colored("Usage", C_CYAN));
    println!(
        "  {} - Run built-in commands directly",
        colored("command", C_GREEN)
    );
    println!(
        "  {} - Run external programs",
        colored("program_name", C_YELLOW)
    );
    println!(
        "  {} - Pipe programs together",
        colored("prog1 | prog2", C_CYAN)
    );
    println!("  {} - Redirect output", colored("prog > file", C_CYAN));
}

#[derive(Debug)]
struct ProcessArguments {
    input: String,
    output: String,
    args_copy: Vec<String>,
    args_addr: Vec<*const u8>,
}

impl ProcessArguments {
    pub fn new(command: &str) -> Self {
        let args: Vec<_> = command.split(' ').collect();
        let mut args_copy: Vec<String> = args
            .iter()
            .filter(|&arg| !arg.is_empty())
            .map(|&arg| {
                let mut string = String::new();
                string.push_str(arg);
                string.push('\0');
                string
            })
            .collect();

        // redirect input
        let mut input = String::new();
        if let Some((idx, _)) = args_copy
            .iter()
            .enumerate()
            .find(|(_, arg)| arg.as_str() == "<\0")
        {
            input = args_copy[idx + 1].clone();
            args_copy.drain(idx..=idx + 1);
        }

        // redirect output
        let mut output = String::new();
        if let Some((idx, _)) = args_copy
            .iter()
            .enumerate()
            .find(|(_, arg)| arg.as_str() == ">\0")
        {
            output = args_copy[idx + 1].clone();
            args_copy.drain(idx..=idx + 1);
        }

        let mut args_addr: Vec<*const u8> = args_copy.iter().map(|arg| arg.as_ptr()).collect();
        args_addr.push(core::ptr::null::<u8>());

        Self {
            input,
            output,
            args_copy,
            args_addr,
        }
    }
}

#[unsafe(no_mangle)]
pub fn main() -> i32 {
    // 打印NimlothOS Logo
    let logo = r#"
 /$$   /$$ /$$               /$$             /$$     /$$        /$$$$$$   /$$$$$$ 
| $$$ | $$|__/              | $$            | $$    | $$       /$$__  $$ /$$__  $$
| $$$$| $$ /$$ /$$$$$$/$$$$ | $$  /$$$$$$  /$$$$$$  | $$$$$$$ | $$  \ $$| $$  \__/
| $$ $$ $$| $$| $$_  $$_  $$| $$ /$$__  $$|_  $$_/  | $$__  $$| $$  | $$|  $$$$$$ 
| $$  $$$$| $$| $$ \ $$ \ $$| $$| $$  \ $$  | $$    | $$  \ $$| $$  | $$ \____  $$
| $$\  $$$| $$| $$ | $$ | $$| $$| $$  | $$  | $$ /$$| $$  | $$| $$  | $$ /$$  \ $$
| $$ \  $$| $$| $$ | $$ | $$| $$|  $$$$$$/  |  $$$$/| $$  | $$|  $$$$$$/|  $$$$$$/
|__/  \__/|__/|__/ |__/ |__/|__/ \______/    \___/  |__/  |__/ \______/  \______/ 
"#;
    println!("{}", colored(logo, &format!("{}{}", C_BOLD, C_MAGENTA)));
    println_info("Enhanced Rust User Shell v1.2 - Type 'help' for available commands.");
    println_info("Use Ctrl+A then X to exit QEMU.");
    println_success("Ready to serve! Try 'test system' to check system status.");
    println!("");
    let mut line: String = String::new();
    print_prompt();
    loop {
        let c = getchar();
        match c {
            LF | CR => {
                println!("");
                if !line.is_empty() {
                    let splited: Vec<_> = line.as_str().split('|').collect();
                    let process_arguments_list: Vec<_> = splited
                        .iter()
                        .map(|&cmd| ProcessArguments::new(cmd))
                        .collect();
                    let mut valid = true;
                    for (i, process_args) in process_arguments_list.iter().enumerate() {
                        if i == 0 {
                            if !process_args.output.is_empty() {
                                valid = false;
                            }
                        } else if i == process_arguments_list.len() - 1 {
                            if !process_args.input.is_empty() {
                                valid = false;
                            }
                        } else if !process_args.output.is_empty() || !process_args.input.is_empty()
                        {
                            valid = false;
                        }
                    }
                    if process_arguments_list.len() == 1 {
                        valid = true;
                    }
                    if !valid {
                        eprintln_error(
                            "Invalid command: Inputs/Outputs cannot be correctly binded!",
                        );
                    } else {
                        // 检查是否为单个内置命令（不支持管道中的内置命令）
                        if process_arguments_list.len() == 1 {
                            let args_copy = &process_arguments_list[0].args_copy;
                            if execute_builtin_command(args_copy) {
                                line.clear();
                                print_prompt();
                                continue;
                            }
                        }
                        // create pipes
                        let mut pipes_fd: Vec<[usize; 2]> = Vec::new();
                        if !process_arguments_list.is_empty() {
                            for _ in 0..process_arguments_list.len() - 1 {
                                let mut pipe_fd = [0usize; 2];
                                pipe(&mut pipe_fd);
                                pipes_fd.push(pipe_fd);
                            }
                        }
                        let mut children: Vec<_> = Vec::new();
                        for (i, process_argument) in process_arguments_list.iter().enumerate() {
                            let pid = fork();
                            if pid == 0 {
                                let input = &process_argument.input;
                                let output = &process_argument.output;
                                let args_copy = &process_argument.args_copy;
                                let args_addr = &process_argument.args_addr;
                                // redirect input
                                if !input.is_empty() {
                                    let input_fd = open(input.as_str(), OpenFlags::RDONLY);
                                    if input_fd == -1 {
                                        eprintln_error(&format!(
                                            "when opening input file: {}",
                                            input.trim_end_matches('\0')
                                        ));
                                        return -4;
                                    }
                                    let input_fd = input_fd as usize;
                                    close(0);
                                    assert_eq!(dup(input_fd), 0);
                                    close(input_fd);
                                }
                                // redirect output
                                if !output.is_empty() {
                                    let output_fd = open(
                                        output.as_str(),
                                        OpenFlags::CREATE | OpenFlags::WRONLY,
                                    );
                                    if output_fd == -1 {
                                        eprintln_error(&format!(
                                            "when opening output file: {}",
                                            output.trim_end_matches('\0')
                                        ));
                                        return -4;
                                    }
                                    let output_fd = output_fd as usize;
                                    close(1);
                                    assert_eq!(dup(output_fd), 1);
                                    close(output_fd);
                                }
                                // receive input from the previous process
                                if i > 0 {
                                    close(0);
                                    let read_end = pipes_fd.get(i - 1).unwrap()[0];
                                    assert_eq!(dup(read_end), 0);
                                }
                                // send output to the next process
                                if i < process_arguments_list.len() - 1 {
                                    close(1);
                                    let write_end = pipes_fd.get(i).unwrap()[1];
                                    assert_eq!(dup(write_end), 1);
                                }
                                // close all pipe ends inherited from the parent process
                                for pipe_fd in pipes_fd.iter() {
                                    close(pipe_fd[0]);
                                    close(pipe_fd[1]);
                                }
                                // execute new application
                                if exec(args_copy[0].as_str(), args_addr.as_slice()) == -1 {
                                    eprintln_error(&format!(
                                        "when executing: {}",
                                        args_copy[0].trim_end_matches('\0')
                                    ));
                                    return -4;
                                }
                                unreachable!();
                            } else {
                                children.push(pid);
                            }
                        }
                        for pipe_fd in pipes_fd.iter() {
                            close(pipe_fd[0]);
                            close(pipe_fd[1]);
                        }
                        let mut exit_code: i32 = 0;
                        for pid in children.into_iter() {
                            let exit_pid = waitpid(pid as usize, &mut exit_code);
                            assert_eq!(pid, exit_pid);
                            //println!("Shell: Process {} exited with code {}", pid, exit_code);
                        }
                    }
                    line.clear();
                }
                print_prompt();
            }
            BS | DL => {
                if !line.is_empty() {
                    print!("{}", BS as char);
                    print!(" ");
                    print!("{}", BS as char);
                    line.pop();
                }
            }
            _ => {
                print!("{}", c as char);
                line.push(c as char);
            }
        }
    }
}
