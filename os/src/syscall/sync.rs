use crate::sync::{Condvar, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task, MutexStatus, SemaphoreStatus};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// 死锁检测函数
/// 
/// 使用银行家算法检测互斥锁和信号量资源分配，判断是否存在死锁。
/// 
/// 返回值：
/// * true - 不存在死锁
/// * false - 存在死锁
fn check() -> bool {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let tasks = &process_inner.tasks;
    let task_count = tasks.len();

    // 检查互斥锁资源分配
    if !{
        let mut available = process_inner.mutex_avail.clone();
        let mut finished = Vec::new();
        finished.resize(task_count, false);
        loop {
            if finished.iter().all(|&f| f) { break true; }
            let mut progress = false;
            for tid in 0..task_count {
                if finished[tid] { continue; }
                let task = process_inner.get_task(tid);
                let task_inner = task.inner_exclusive_access();
                // 判断该任务是否被阻塞
                if (0..process_inner.mutex_list.len()).all(|mid| task_inner.mutex_status[mid].needed <= available[mid]) {
                    // 可以执行，释放资源
                    finished[tid] = true;
                    progress = true;
                    for mid in 0..process_inner.mutex_list.len() {
                        available[mid] += task_inner.mutex_status[mid].allocated;
                    }
                }
            }
            if !progress { break false; }
        }
    } {
        return false;
    }

    // 检查信号量资源分配
    {
        let mut available = process_inner.semaphore_avail.clone();
        let mut finished = Vec::new();
        finished.resize(task_count, false);
        loop {
            if finished.iter().all(|&f| f) { return true; }
            let mut progress = false;
            for tid in 0..task_count {
                if finished[tid] { continue; }
                let task = process_inner.get_task(tid);
                let task_inner = task.inner_exclusive_access();
                if (0..process_inner.semaphore_list.len()).all(|sid| task_inner.semaphore_status[sid].needed <= available[sid]) {
                    finished[tid] = true;
                    progress = true;
                    for sid in 0..process_inner.semaphore_list.len() {
                        available[sid] += task_inner.semaphore_status[sid].allocated;
                    }
                }
            }
            if !progress { return false; }
        }
    }
}

/// 睡眠系统调用
/// 
/// 参数：
/// * ms - 睡眠时间（毫秒）
/// 
/// 返回值：
/// * 0 - 成功
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    // 计算到期时间并添加定时器
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

/// 创建互斥锁系统调用
/// 
/// 参数：
/// * blocking - 是否创建阻塞式互斥锁
/// 
/// 返回值：
/// * >= 0 - 互斥锁ID
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 优先复用空闲互斥锁槽，否则扩展新槽
    let id = process_inner
        .mutex_list
        .iter()
        .position(|item| item.is_none())
        .unwrap_or_else(|| {
            process_inner.mutex_list.push(None);
            process_inner.mutex_avail.push(0);
            // 为所有任务添加新的互斥锁状态
            for tid in 0..process_inner.tasks.len() {
                let task = process_inner.get_task(tid);
                task.inner_exclusive_access().mutex_status.push(MutexStatus { allocated: 0, needed: 0 });
            }
            process_inner.mutex_list.len() - 1
        });

    // 初始化所有任务的互斥锁状态
    for tid in 0..process_inner.tasks.len() {
        let task = process_inner.get_task(tid);
        task.inner_exclusive_access().mutex_status[id].clear();
    }
    // 创建互斥锁对象
    process_inner.mutex_list[id] = if !blocking {
        Some(Arc::new(MutexSpin::new(id)))
    } else {
        Some(Arc::new(MutexBlocking::new(id)))
    };
    process_inner.mutex_avail[id] = 1;
    id as isize
}

/// 锁定互斥锁系统调用
/// 
/// 参数：
/// * mutex_id - 互斥锁ID
/// 
/// 返回值：
/// * 0 - 成功
/// * -0xdead - 死锁检测失败
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let detect = process_inner.deadlock_detect;
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());

    // 标记当前任务需要该互斥锁
    {
        let task = current_task().unwrap();
        task.inner_exclusive_access().mutex_status[mutex_id].need_set();
    }
    drop(process_inner);
    drop(process);

    // 死锁检测（如启用）
    if detect && !check() {
        return -0xdead;
    }

    // 获取互斥锁
    mutex.lock();
    0
}

/// 解锁互斥锁系统调用
/// 
/// 参数：
/// * mutex_id - 互斥锁ID
/// 
/// 返回值：
/// * 0 - 成功
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}

/// 创建信号量系统调用
/// 
/// 参数：
/// * res_count - 初始资源数量
/// 
/// 返回值：
/// * >= 0 - 信号量ID
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 优先复用空闲信号量槽，否则扩展新槽
    let id = process_inner
        .semaphore_list
        .iter()
        .position(|item| item.is_none())
        .unwrap_or_else(|| {
            process_inner.semaphore_list.push(None);
            process_inner.semaphore_avail.push(0);
            // 为所有任务添加新的信号量状态
            for tid in 0..process_inner.tasks.len() {
                let task = process_inner.get_task(tid);
                task.inner_exclusive_access().semaphore_status.push(SemaphoreStatus { allocated: 0, needed: 0 });
            }
            process_inner.semaphore_list.len() - 1
        });

    // 初始化所有任务的信号量状态
    for tid in 0..process_inner.tasks.len() {
        let task = process_inner.get_task(tid);
        task.inner_exclusive_access().semaphore_status[id].clear();
    }
    // 创建信号量对象
    process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count, id)));
    process_inner.semaphore_avail[id] = res_count;
    id as isize
}

/// 信号量up操作系统调用
/// 
/// 参数：
/// * sem_id - 信号量ID
/// 
/// 返回值：
/// * 0 - 成功
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    0
}

/// 信号量down操作系统调用
/// 
/// 参数：
/// * sem_id - 信号量ID
/// 
/// 返回值：
/// * 0 - 成功
/// * -0xdead - 死锁检测失败
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    // 获取进程和信号量资源
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let detect = process_inner.deadlock_detect;
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());

    // 死锁检测步骤：
    // 1. 临时标记当前任务需要该信号量
    {
        let task = current_task().unwrap();
        task.inner_exclusive_access().semaphore_status[sem_id].need_set();
    }
    drop(process_inner);
    drop(process);

    // 2. 如果启用了死锁检测，检查是否会发生死锁
    if detect && !check() { 
        // 检测到可能发生死锁，返回错误码
        return -0xdead; 
    }

    // 3. 取消临时标记
    {
        let task = current_task().unwrap();
        task.inner_exclusive_access().semaphore_status[sem_id].need_unset();
    }
    
    // 4. 尝试获取信号量资源
    sem.down();
    0
}

/// 创建条件变量系统调用
/// 
/// 返回值：
/// * >= 0 - 条件变量ID
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    
    // 查找空闲的条件变量槽位或创建新的条件变量
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}

/// 条件变量signal操作系统调用
/// 
/// 参数：
/// * condvar_id - 条件变量ID
/// 
/// 返回值：
/// * 0 - 成功
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}

/// 条件变量wait操作系统调用
/// 
/// 参数：
/// * condvar_id - 条件变量ID
/// * mutex_id - 互斥锁ID
/// 
/// 返回值：
/// * 0 - 成功
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}

/// 启用死锁检测系统调用
/// 
/// 参数：
/// * _enabled - 是否启用死锁检测（1表示启用，0表示禁用）
/// 
/// 返回值：
/// * 0 - 成功
pub fn sys_enable_deadlock_detect(_enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect NOT IMPLEMENTED");
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.deadlock_detect = _enabled == 1;
    0
}
