首先介绍一下几个概念。

SBI是RISC-V Supervisor Binary Interface的缩写，OpenSBI是RISC-V官方用C语言开发的SBI参考实现，RustSBI是用Rust实现的SBI。

机器在上电以后，会从ROM中读取代码，然后引导整个计算机软硬件系统的启动，而整个启动过程分为多个阶段，目前通用的多阶段引导模型是：

ROM -> LOADER -> RUNTIME -> BOOTLOADER -> OS

Loader进行内存初始化，并加载Runtime和BootLoader程序，同时LOADER也是一段程序，常见的LOADER有BIOS和UEFI。

Runtime固件程序是为了提供运行时服务，它是对硬件最基础的抽象，对OS提供服务。SBI就是RISC-V架构的Runtime规范。

BootLoader要进行文件系统引导、网卡引导、操作系统启动配置项设置、操作系统加载等，常见的BootLoader包括GRUB、U-Boot、LinuxBoot等。

不过BIOS和UEFI的大部分实现都是Loader、Runtime、BootLoader三合一的。

在[rustsbi/rustsbi](https://github.com/rustsbi/rustsbi)的`sbi_rt`部分封装了调用SBI服务的接口，不过这里我使用的是新版本的RustSBI，接口是自己写的，详见`sbi.rs`文件。

然后在`console`文件中实现了`core::fmt::Write trait`的一些方法和`print!`、`println!`宏。

错误处理在`lang_item.rs`中。