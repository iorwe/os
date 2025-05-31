//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    index: usize,
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize, id: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
            index: id,
        }
    }

    /// 信号量up操作
    ///
    /// 增加信号量计数，若有等待任务则唤醒队首任务，否则更新进程信号量可用数
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        let cur_task = current_task().unwrap();
        let id = self.index;

        // 增加信号量计数
        inner.count += 1;

        if inner.count <= 0 {
            // 有等待任务，唤醒队首
            if let Some(task) = inner.wait_queue.pop_front() {
                {
                    let mut task_inner = task.as_ref().inner_exclusive_access();
                    task_inner.semaphore_status[id].semaphore_borrowed();
                }
                wakeup_task(task);
            }
        } else {
            // 没有等待任务，直接更新进程信号量可用数
            let process = cur_task.process.upgrade().unwrap();
            process.inner_exclusive_access().semaphore_avail[id] += 1;
        }
    }

    /// 信号量的down操作（获取资源）
    /// 
    /// 减少信号量计数，若资源不足则阻塞当前任务，否则更新进程和任务状态
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let mut inner = self.inner.exclusive_access();
        let cur_task = current_task().unwrap();
        let id = self.index;

        // 减少信号量计数
        inner.count -= 1;

        if inner.count < 0 {
            // 资源不足，将当前任务加入等待队列并阻塞
            inner.wait_queue.push_back(cur_task.clone());
            cur_task.inner_exclusive_access().semaphore_status[id].need_set();
            drop(inner);
            block_current_and_run_next();
        } else {
            // 成功获取资源，更新进程和任务状态
            let process = cur_task.process.upgrade().unwrap();
            process.inner_exclusive_access().semaphore_avail[id] -= 1;
            cur_task.inner_exclusive_access().semaphore_status[id].allocate_set();
        }
    }
}
