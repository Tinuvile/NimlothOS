.altmacro
.macro SAVE_GP n
   sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
    .section .text.trampoline
    .global __alltraps
    .global __restore
    .align 2

__alltraps:
    # 交换 sscratch and sp(x2) 寄存器内容
    csrrw sp, sscratch, sp
    # 现在sp->kernel stack，sscratch->user stack
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
    # 保存kernel_satp到t0
    ld t0, 34*8(sp)
    # 保存trap_handler到t1
    ld t1, 36*8(sp)
    # 跳转到kernel_sp
    ld sp, 35*8(sp)
    # 切换到内核地址空间
    csrw satp, t0
    sfence.vma
    # 跳转到trap_handler
    jr t1

__restore:
    # 切换到用户地址空间
    csrw satp, a1
    sfence.vma
    csrw sscratch, a0
    mv sp, a0
    # 恢复CSR和通用寄存器
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
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
    # 恢复用户栈指针
    ld sp, 2*8(sp)
    sret