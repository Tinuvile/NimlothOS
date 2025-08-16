#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

// 创建多层函数调用来测试栈回溯
fn function_a() {
    println!("In function_a, calling function_b");
    function_b();
}

fn function_b() {
    println!("In function_b, calling function_c");
    function_c();
}

fn function_c() {
    println!("In function_c, about to trigger panic");
    panic!("Test panic for stack trace!");
}

// 递归函数测试
fn recursive_panic(n: i32) {
    println!("Recursive call: n = {}", n);
    if n <= 0 {
        panic!("Panic from recursive function at depth 0!");
    }
    recursive_panic(n - 1);
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    println!("Stack Trace Test Program");
    println!("========================");

    println!("This program will test stack tracing by triggering panics");

    // 测试1: 通过嵌套函数调用触发 panic
    println!("\nTest 1: Nested function calls");
    function_a();

    // 这行代码不会执行，因为上面的 panic 会终止程序
    println!("This should not be printed");

    0
}
