//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
    index: usize,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new(_id: usize) -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
            index: _id
        }
    }
}

impl Mutex for MutexSpin {
    /// 获取自旋锁
    ///
    /// 尝试获取锁，若被占用则挂起当前任务，否则更新状态
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        loop {
            let id = self.index;
            let task = current_task().unwrap();
            let mut locked = self.locked.exclusive_access();
            let mut task_inner = task.inner_exclusive_access();
            if *locked {
                // 锁被占用，挂起当前任务
                drop(locked);
                task_inner.mutex_status[id].mutex_requested();
                drop(task_inner);
                suspend_current_and_run_next();
            } else {
                // 成功获取锁，更新状态
                *locked = true;
                task_inner.mutex_status[id].mutex_borrowed();
                let process = task.process.upgrade().unwrap();
                process.inner_exclusive_access().mutex_avail[id] = 0;
                return;
            }
        }
    }

    /// 释放自旋锁
    ///
    /// 释放锁并更新任务和进程状态
    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let id = self.index;
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        let task = current_task().unwrap();
        task.inner_exclusive_access().mutex_status[id].allocate_unset();
        let process = task.process.upgrade().unwrap();
        process.inner_exclusive_access().mutex_avail[id] = 1;
    }
}

/// 阻塞式互斥锁结构体
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
    index: usize
}

/// 阻塞式互斥锁内部状态
pub struct MutexBlockingInner {
    /// 锁的占用状态
    locked: bool,
    /// 等待队列，存储等待获取锁的任务
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// 创建一个新的阻塞式互斥锁
    /// 
    /// # 参数
    /// * `_id` - 互斥锁的唯一标识符
    pub fn new(_id: usize) -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            index: _id,
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// 获取阻塞式互斥锁
    ///
    /// 若锁被占用则将当前任务加入等待队列并阻塞，否则直接获取锁
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();
        let task = current_task().unwrap();
        let mut task_inner = task.inner_exclusive_access();
        let id = self.index;
        if mutex_inner.locked {
            // 锁被占用，加入等待队列并阻塞
            mutex_inner.wait_queue.push_back(task.clone());
            drop(mutex_inner);
            task_inner.mutex_status[id].mutex_requested();
            drop(task_inner);
            block_current_and_run_next();
        } else {
            // 成功获取锁，更新状态
            mutex_inner.locked = true;
            task_inner.mutex_status[id].mutex_borrowed();
            let process = task.process.upgrade().unwrap();
            process.inner_exclusive_access().mutex_avail[id] = 0;
        }
    }

    /// 释放阻塞式互斥锁
    ///
    /// 若有等待任务则唤醒队首，否则释放锁并更新状态
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        let id = self.index;
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            {
                let mut task_inner = waking_task.inner_exclusive_access();
                task_inner.mutex_status[id].mutex_borrowed();
            }
            wakeup_task(waking_task);
        } else {
            // 没有等待任务，释放锁并更新状态
            mutex_inner.locked = false;
            let task = current_task().unwrap();
            task.inner_exclusive_access().mutex_status[id].allocate_unset();
            let process = task.process.upgrade().unwrap();
            process.inner_exclusive_access().mutex_avail[id] = 1;
        }
    }
}
