RustSBI运行在Machine特权级，

```bash
tinuvile@LAPTOP-7PVP3HH3:~/NimlothOS/os$ cargo build --release
warning: constant `SBI_CONSOLE_GETCHAR` is never used
 --> src/sbi.rs:7:7
  |
7 | const SBI_CONSOLE_GETCHAR: usize = 2;
  |       ^^^^^^^^^^^^^^^^^^^
  |
  = note: `#[warn(dead_code)]` on by default

warning: constant `SBI_CLEAR_IPI` is never used
 --> src/sbi.rs:8:7
  |
8 | const SBI_CLEAR_IPI: usize = 3;
  |       ^^^^^^^^^^^^^

warning: constant `SBI_SEND_IPI` is never used
 --> src/sbi.rs:9:7
  |
9 | const SBI_SEND_IPI: usize = 4;
  |       ^^^^^^^^^^^^

warning: constant `SBI_REMOTE_FENCE_I` is never used
  --> src/sbi.rs:10:7
   |
10 | const SBI_REMOTE_FENCE_I: usize = 5;
   |       ^^^^^^^^^^^^^^^^^^

warning: constant `SBI_REMOTE_SFENCE_VMA` is never used
  --> src/sbi.rs:11:7
   |
11 | const SBI_REMOTE_SFENCE_VMA: usize = 6;
   |       ^^^^^^^^^^^^^^^^^^^^^

warning: constant `SBI_REMOTE_SFENCE_VMA_ASID` is never used
  --> src/sbi.rs:12:7
   |
12 | const SBI_REMOTE_SFENCE_VMA_ASID: usize = 7;
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: `os` (bin "os") generated 6 warnings
    Finished `release` profile [optimized + debuginfo] target(s) in 0.01s
tinuvile@LAPTOP-7PVP3HH3:~/NimlothOS/os$ rust-objcopy --strip-all target/riscv64gc-unknown-none-elf/release/os -O binary target/riscv64gc-unknown-none-elf/release/os.bin
tinuvile@LAPTOP-7PVP3HH3:~/NimlothOS/os$ qemu-system-riscv64 -machine virt -nographic -bios ../bootloader/rus
tsbi-qemu.bin -device loader,file=target/riscv64gc-unknown-none-elf/release/os.bin,addr=0x80200000
[rustsbi] RustSBI version 0.3.1, adapting to RISC-V SBI v1.0.0
.______       __    __      _______.___________.  _______..______   __
|   _  \     |  |  |  |    /       |           | /       ||   _  \ |  |
|  |_)  |    |  |  |  |   |   (----`---|  |----`|   (----`|  |_)  ||  |
|      /     |  |  |  |    \   \       |  |      \   \    |   _  < |  |
|  |\  \----.|  `--'  |.----)   |      |  |  .----)   |   |  |_)  ||  |
| _| `._____| \______/ |_______/       |__|  |_______/    |______/ |__|
[rustsbi] Implementation     : RustSBI-QEMU Version 0.2.0-alpha.2
[rustsbi] Platform Name      : riscv-virtio,qemu
[rustsbi] Platform SMP       : 1
[rustsbi] Platform Memory    : 0x80000000..0x88000000
[rustsbi] Boot HART          : 0
[rustsbi] Device Tree Region : 0x87e00000..0x87e0107e
[rustsbi] Firmware Address   : 0x80000000
[rustsbi] Supervisor Address : 0x80200000
[rustsbi] pmp01: 0x00000000..0x80000000 (-wr)
[rustsbi] pmp02: 0x80000000..0x80200000 (---)
[rustsbi] pmp03: 0x80200000..0x88000000 (xwr)
[rustsbi] pmp04: 0x88000000..0x00000000 (-wr)
Hello, world!
Paniced at src/main.rs:15:Shutdown machine!
```