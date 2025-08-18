.altmacro
.macro SAVE_GP n
   sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
.macro SAVE_FP n
    fsd f\n, (34+\n)*8(sp)
.endm
.macro LOAD_FP n
    fld f\n, (34+\n)*8(sp)
.endm
    .section .text
    # 声明架构特性，显式开启F/D拓展
    .option push
    .option arch, +f, +d
    .global __alltraps
    .global __restore
    .align 2

__alltraps:
    # 交换 sscratch and sp(x2) 寄存器内容
    csrrw sp, sscratch, sp
    # 现在sp->kernel stack，sscratch->user stack
    # 分配栈帧
    addi sp, sp, -67*8
    # 保存通用寄存器，跳过x0,tp(x4)
    sd x1, 1*8(sp)
    sd x3, 3*8(sp)
    .set n, 5
    .rept 27
        SAVE_GP %n
        .set n, n+1
    .endr
    # 读寄存器并保存
    csrr t0, sstatus
    csrr t1, sepc
    sd t0, 32*8(sp)
    sd t1, 33*8(sp)
    # sscratch 现在指向用户栈
    csrr t2, sscratch
    sd t2, 2*8(sp)
    # 置 fs = dirty，允许内核使用浮点指令
    li t4, (3 << 13)
    csrs sstatus, t4
    # 保存 fcsr 到 0
    frcsr t3
    sd t3, 66*8(sp)
    # 浮点寄存器
    .set n, 0
    .rept 32
        SAVE_FP %n
        .set n, n+1
    .endr
    # a0指向内核栈的栈指针，trap_handler从中获取cx参数
    mv a0, sp
    call trap_handler

__restore:
    # mv sp, a0
    # 恢复CSR和通用寄存器
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    ld t2, 2*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    csrw sscratch, t2
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # 置 fs = dirty 使用 fld
    li t4, (3 << 13)
    csrs sstatus, t4
    # 恢复 fcsr
    ld t3, 66*8(sp)
    fscsr t3
    .set n, 0
    .rept 32
        LOAD_FP %n
        .set n, n+1
    .endr
    # 回收陷入帧
    addi sp, sp, 67*8
    # 交换 sscratch and sp
    csrrw sp, sscratch, sp
    # 现在sp->user stack，sscratch->kernel stack
    sret
    .option pop