.altmacro
.macro SAVE_SN n
    sd s\n, (\n+2)*8(a0)
.endm
.macro LOAD_SN n
    ld s\n, (\n+2)*8(a1)
.endm
    .section .text
    .global __switch
__switch:
    # __switch(
    #     current_process_cx_ptr: *mut TrapContext,
    #     next_process_cx_ptr: *mut TrapContext,
    # )
    # current_process_cx_ptr在a0，next_process_cx_ptr在a1
    # 保存当前进程状态
    sd sp, 8(a0)
    sd ra, 0(a0)
    .set n, 0
    .rept 12
        SAVE_SN %n
        .set n, n+1
    .endr
    # 恢复下一个进程状态
    ld ra, 0(a1)
    .set n, 0
    .rept 12
        LOAD_SN %n
        .set n, n+1
    .endr
    ld sp, 8(a1)
    ret
