# [¶chapter3练习](https://learningos.cn/rCore-Tutorial-Guide-2025S/chapter3/5exercise.html#chapter3)

*2021011919 李声强*

## 实现的功能

- 在本次实现中，为操作系统添加了新的系统调用sys_trace（ID 410），用于追踪任务系统调用的历史信息。具体功能包括：
  - 在TaskControlBlock中增加了syscall_count数组，用于记录每个任务的系统调用使用次数
  - 在TaskManager中添加了管理系统调用计数的方法
  - 在任务模块中暴露了相应的公共API接口
  - 修改syscall函数，每次系统调用都会自动记录次数
  - 实现了sys_trace系统调用的三种功能：
    - 从指定地址读取一个字节
    - 向指定地址写入一个字节
    - 查询特定系统调用的历史调用次数

## 简答题

### 1.

#### 程序出错行为：

- ch2b_bad_address：PageFault in application, kernel killed it.
- ch2b_bad_instructions：IllegalInstruction in application, kernel killed it
- ch2b_bad_register：IllegalInstruction in application, kernel killed it

（使用的 sbi ：RustSBI version 0.3.0-alpha.4, adapting to RISC-V SBI v1.0.0）



### 2

#### （1）

##### 刚进入__restore时，sp代表的值：

sp指向内核栈上的TrapContext结构体的起始地址。TrapContext包含了被保存的用户程序执行状态，包括通用寄存器、sstatus和sepc等。

##### __restore的两种使用情景：

1. 从trap_handler返回用户空间：

当发生中断或异常时，进入__alltraps，它将上下文保存到内核栈并调用trap_handler函数。在trap_handler处理完成后，返回值是指向TrapContext的指针，程序随后进入 __restore来恢复这个上下文。这种情况下，__restore恢复被中断的用户程序的执行状态，包括所有通用寄存器、sstatus和sepc等。具体可以在trap_handler函数中看到这个流程。

2. 任务切换时初始化新任务：

当系统初始化一个新任务时，通过TaskContext::goto_restore函数创建一个特殊的TaskContext，其中ra字段被设置为__restore的地址。当这个任务被调度执行时，__switch函数会加载这个TaskContext，使程序流跳转到__restore。在这种情况下，sp指向的是提前准备好的TrapContext（在kstack_ptr处），__restore会从这个TrapContext恢复状态，然后通过sret指令进入用户态开始执行任务。

#### （2）

1. sstatus寄存器 (ld t0, 32*8(sp) 和 csrw sstatus, t0)：

   - sstatus是RISC-V中的特权级状态控制寄存器

   - 对于用户态的意义：

     - SPP位(Supervisor Previous Privilege)决定sret指令返回的特权级别，设为0表示返回到U模式

     - SPIE位(Supervisor Previous Interrupt Enable)控制返回后的中断使能状态

     - SIE位(Supervisor Interrupt Enable)控制S模式中断是否启用

     - 恢复sstatus确保了程序返回时使用正确的特权级别和中断设置

2. sepc寄存器 (ld t1, 33*8(sp) 和 csrw sepc, t1)：

   - sepc是异常程序计数器(Supervisor Exception Program Counter)

   - 对于用户态的意义：

     - 保存了用户程序应该继续执行的指令地址

     - sret指令执行时，处理器会跳转到sepc中存储的地址

     - 对于系统调用，这通常是ecall指令之后的地址

     - 对于初次进入用户程序，这是程序的入口点

3. 用户栈指针 (ld t2, 2*8(sp) 和 csrw sscratch, t2)：

   - 这组操作加载并设置了用户栈指针

   - 对于用户态的意义：

     - sscratch寄存器在切换特权级时用于保存临时值，sscratch存储了用户栈的指针

     - 当异常发生时，会交换sp和sscratch，实现内核栈和用户栈的快速切换

     - 正确设置sscratch确保了程序返回用户态后能够访问正确的用户栈

这些寄存器的设置是特权级切换机制的核心部分，确保了：

- 程序返回到正确的特权级别(U模式)

- 从正确的指令地址继续执行

- 使用正确的栈空间

- 维护适当的中断状态

#### （3）

1. x2 (sp) 寄存器：

- x2是栈指针寄存器(sp)

- 没有直接从TrapContext恢复sp，而是通过特殊处理：在函数最后使用csrrw sp, sscratch, sp指令将sp与sscratch交换

- 因为在内核态时，sp指向内核栈；而返回用户态需要sp指向用户栈，用户栈的值之前已被加载到sscratch寄存器

2. x4 (tp) 寄存器：

- x4是线程指针寄存器

- 在当前操作系统设计中，tp寄存器是为线程本地存储(TLS)预留的，由于当前阶段的应用程序还没有使用tp寄存器的需求，所以跳过了对它的保存和恢复

#### （4）

sp中的值：

- 意义：此时sp指向用户栈的顶部，使得用户程序可以正确访问自己的栈空间

sscratch中的值：

- 意义：保存了当前任务的内核栈位置，为下一次从用户态进入内核态做准备

#### （5）

sret指令是状态切换的关键指令。执行该指令后会进入用户态：

1. 特权级别切换机制：

- sret指令专门用于从S模式返回到较低特权级，它会读取sstatus寄存器中的SPP位来决定返回的特权级别，在现在的代码中，sstatus已被设置为SPP=0，表示返回到U模式

2. 程序计数器的变更：

- sret指令执行时，会将sepc寄存器的值加载到PC中，之前代码已经将sepc设置为用户程序应该执行的地址

3. 状态寄存器的更新：

- sret执行时会自动修改sstatus寄存器：

  - SPP位被清零

  - SPIE位的值被复制到SIE位

  - SPIE位被设为1

- 这些更改确保了正确的特权级别和中断状态

#### （6）

sp中的值：

- 包含了原来sscratch中存储的内核栈指针

- 意义：此时sp指向当前任务的内核栈空间，为保存用户上下文和执行内核代码做准备

sscratch中的值：

- 包含了原来sp中存储的用户栈指针

- 意义：保存用户程序的栈指针，以便在内核处理完成后能够恢复用户程序的执行环境

#### （7）

从U态进入S态是通过ecall指令发生的。

当用户程序需要请求操作系统服务时，它会执行ecall指令。

- 硬件自动将当前的PC保存到sepc寄存器
- 硬件将异常原因写入scause寄存器
- 硬件将特权级别从U模式提升到S模式
- 硬件将控制流转移到stvec寄存器指向的地址（现在的操作系统将其设置为__alltraps函数的地址）



# **荣誉准则**

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：无

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：无

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。