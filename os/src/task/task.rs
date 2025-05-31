//! Types related to task management & Functions for completely changing TCB

use super::id::TaskUserRes;
use super::{kstack_alloc, KernelStack, ProcessControlBlock, TaskContext};
use crate::trap::TrapContext;
use crate::{mm::PhysPageNum, sync::UPSafeCell};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;

/// 任务控制块结构体
/// 
/// 用于管理单个任务（线程）的所有信息
pub struct TaskControlBlock {
    /// 指向所属进程的弱引用
    pub process: Weak<ProcessControlBlock>,
    /// 内核栈
    pub kstack: KernelStack,
    /// 任务控制块内部数据
    inner: UPSafeCell<TaskControlBlockInner>,
}

impl TaskControlBlock {
    /// 获取任务控制块内部数据的可变引用
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }

    /// 获取用户态页表的地址
    pub fn get_user_token(&self) -> usize {
        let process = self.process.upgrade().unwrap();
        let inner = process.inner_exclusive_access();
        inner.memory_set.token()
    }
}

/// 互斥锁状态结构体
/// 
/// 用于记录任务对互斥锁的分配和需求状态
pub struct MutexStatus {
    /// 已分配的互斥锁数量（0或1）
    pub allocated: usize,
    /// 需要的互斥锁数量（0或1）
    pub needed: usize,
}

impl MutexStatus {
    /// 标记互斥锁已分配
    pub fn allocate_set(&mut self) { self.allocated = 1; }

    /// 标记互斥锁未分配
    pub fn allocate_unset(&mut self) { self.allocated = 0; }

    /// 标记需要互斥锁
    pub fn need_set(&mut self) { self.needed = 1; }

    /// 标记不需要互斥锁
    pub fn need_unset(&mut self) { self.needed = 0; }

    /// 清除互斥锁状态
    pub fn clear(&mut self) { 
        self.allocated = 0; 
        self.needed = 0; 
    }

    /// 标记互斥锁已借出（已分配且不需要）
    pub fn mutex_borrowed(&mut self) {
        self.allocate_set();
        self.need_unset();
    }

    /// 标记互斥锁已请求（未分配但需要）
    pub fn mutex_requested(&mut self) {
        self.allocate_unset();
        self.need_set();
    }
}

/// 信号量状态结构体
/// 
/// 用于记录任务对信号量的分配和需求状态
pub struct SemaphoreStatus {
    /// 已分配的信号量资源数量
    pub allocated: usize,
    /// 需要的信号量资源数量
    pub needed: usize,
}

impl SemaphoreStatus {
    /// 增加已分配的信号量资源数量
    pub fn allocate_set(&mut self) { self.allocated += 1; }

    /// 减少已分配的信号量资源数量
    pub fn allocate_unset(&mut self) { self.allocated -= 1; }

    /// 增加需要的信号量资源数量
    pub fn need_set(&mut self) { self.needed += 1; }

    /// 减少需要的信号量资源数量
    pub fn need_unset(&mut self) { self.needed -= 1; }

    /// 清除信号量状态
    pub fn clear(&mut self) { 
        self.allocated = 0; 
        self.needed = 0; 
    }

    /// 标记信号量资源已借出（已分配且不需要）
    pub fn semaphore_borrowed(&mut self) {
        self.allocate_set();
        self.need_unset();
    }

    /// 标记信号量资源已请求（未分配但需要）
    pub fn semaphore_requested(&mut self) {
        self.allocate_unset();
        self.need_set();
    }
}

/// 任务控制块内部数据结构
pub struct TaskControlBlockInner {
    /// 用户态资源
    pub res: Option<TaskUserRes>,
    /// 存放trap上下文的物理页号
    pub trap_cx_ppn: PhysPageNum,
    /// 任务上下文
    pub task_cx: TaskContext,

    /// 当前任务的执行状态
    pub task_status: TaskStatus,
    /// 任务退出码（在主动退出或执行出错时设置）
    pub exit_code: Option<i32>,

    /// 互斥锁状态数组
    pub mutex_status: Vec<MutexStatus>,
    /// 信号量状态数组
    pub semaphore_status: Vec<SemaphoreStatus>,
}

impl TaskControlBlockInner {
    /// 获取trap上下文的可变引用
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }

    #[allow(unused)]
    /// 获取当前任务状态
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
}

impl TaskControlBlock {
    /// 创建新任务
    /// 
    /// 参数：
    /// * process - 所属进程
    /// * ustack_base - 用户栈基址
    /// * alloc_user_res - 是否分配用户态资源
    pub fn new(
        process: Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        // 创建用户态资源
        let res = TaskUserRes::new(Arc::clone(&process), ustack_base, alloc_user_res);
        let trap_cx_ppn = res.trap_cx_ppn();
        let kstack = kstack_alloc();
        let kstack_top = kstack.get_top();

        // 创建任务控制块
        Self {
            process: Arc::downgrade(&process),
            kstack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    res: Some(res),
                    trap_cx_ppn,
                    task_cx: TaskContext::goto_trap_return(kstack_top),
                    task_status: TaskStatus::Ready,
                    exit_code: None,
                    mutex_status: Vec::new(),
                    semaphore_status: Vec::new(),
                })
            },
        }
    }
}

/// 任务执行状态枚举
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// 就绪状态，等待调度
    Ready,
    /// 运行状态，正在执行
    Running,
    /// 阻塞状态，等待资源或事件
    Blocked,
}
