//! Concurrency: share-nothing tasks, a work-stealing pool, structured scopes and channels.
//!
//! Every task runs in its own interpreter instance. Arguments, results and channel messages are
//! *copied* (`Data`), which matches Fusion's rule that values crossing task or actor boundaries
//! are copied or immutable.
//!
//! * `spawn` — one OS thread per task; use for tasks that block on channels, sockets or processes.
//! * `pool_spawn`, `parallel_map`, scopes — a work-stealing pool (per-worker deques, a global
//!   injector queue, stealing from the front of other workers' deques, and helping while waiting).
//! * `scope_*` — structured concurrency: a scope waits for all of its children.
//!
//! Not implemented: preemption, async/await-aware scheduling of suspended tasks, or sharing
//! mutable values between tasks.

use crate::ast::Program;
use crate::data_ext::Data;
use crate::interpreter::Interpreter;
use crate::stdlib_ext::{data_to_value, value_to_data};
use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const STACK_SIZE: usize = 64 * 1024 * 1024;

/// A finished task: its result value and the lines it printed.
pub type TaskOutcome = Result<(Data, Vec<String>), String>;

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

pub fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "task panicked".to_string()
    }
}

pub fn cpu_count() -> usize {
    thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn run_function(interp: &mut Interpreter, name: &str, args: Vec<Data>) -> TaskOutcome {
    if !interp.has_function(name) {
        return Err(format!("no function named '{}'", name));
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let values = args.into_iter().map(data_to_value).collect();
        let value = interp.call_function_by_name(name, values);
        value_to_data(&value)
    }));
    let lines = interp.take_output();
    match result {
        Ok(Ok(data)) => Ok((data, lines)),
        Ok(Err(e)) => Err(format!("task '{}' returned a value that cannot cross a task boundary: {}", name, e)),
        Err(payload) => Err(panic_message(payload)),
    }
}

fn tasks() -> &'static Mutex<HashMap<i64, JoinHandle<TaskOutcome>>> {
    static TASKS: OnceLock<Mutex<HashMap<i64, JoinHandle<TaskOutcome>>>> = OnceLock::new();
    TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Runs `name(args...)` to completion in a fresh interpreter.
fn execute_task(program: Arc<Program>, name: &str, args: Vec<Data>) -> TaskOutcome {
    let mut interp = Interpreter::new();
    interp.attach_program(program.clone());
    if catch_unwind(AssertUnwindSafe(|| interp.register_declarations(&program))).is_err() {
        return Err("could not initialise task interpreter".to_string());
    }
    run_function(&mut interp, name, args)
}

/// Starts `name(args...)` on its own OS thread and returns a task id for `task_join`.
/// Use this for tasks that block (waiting on channels, sockets or processes).
pub fn spawn(program: Arc<Program>, name: String, args: Vec<Data>) -> Result<i64, String> {
    let handle = thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || execute_task(program, &name, args))
        .map_err(|e| format!("cannot start task thread: {}", e))?;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    tasks().lock().unwrap().insert(id, handle);
    Ok(id)
}

// ---------------------------------------------------------------------------
// Work-stealing pool
// ---------------------------------------------------------------------------
//
// Every worker owns a deque. A worker pushes the jobs it creates onto the back of its own deque
// and pops from the back (newest first, good locality); idle workers take from the global
// injector queue and otherwise steal from the *front* of other workers' deques (oldest first).
// A task that waits for another task keeps executing queued jobs instead of blocking its thread,
// so nested fork/join cannot exhaust the pool. Jobs that block on channels, sockets or processes
// should use `spawn` (one OS thread each) instead.

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Pool {
    injector: Mutex<VecDeque<Job>>,
    locals: Vec<Mutex<VecDeque<Job>>>,
    wake: (Mutex<()>, Condvar),
}

thread_local! {
    static WORKER_INDEX: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<&'static Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let workers = cpu_count().max(2);
        let pool: &'static Pool = Box::leak(Box::new(Pool {
            injector: Mutex::new(VecDeque::new()),
            locals: (0..workers).map(|_| Mutex::new(VecDeque::new())).collect(),
            wake: (Mutex::new(()), Condvar::new()),
        }));
        for index in 0..workers {
            let _ = thread::Builder::new().name(format!("fusion-worker-{}", index)).stack_size(STACK_SIZE).spawn(move || {
                WORKER_INDEX.with(|w| w.set(Some(index)));
                loop {
                    match pool.find_job(Some(index)) {
                        Some(job) => run_job(job),
                        None => {
                            let guard = pool.wake.0.lock().unwrap();
                            let _ = pool.wake.1.wait_timeout(guard, Duration::from_millis(5)).unwrap();
                        }
                    }
                }
            });
        }
        pool
    })
}

fn run_job(job: Job) {
    // A panicking job must not take its worker down; task jobs already catch panics themselves.
    let _ = catch_unwind(AssertUnwindSafe(job));
}

impl Pool {
    fn find_job(&self, me: Option<usize>) -> Option<Job> {
        if let Some(i) = me {
            if let Some(job) = self.locals[i].lock().unwrap().pop_back() {
                return Some(job);
            }
        }
        if let Some(job) = self.injector.lock().unwrap().pop_front() {
            return Some(job);
        }
        let n = self.locals.len();
        let start = me.unwrap_or(0);
        for k in 1..=n {
            let victim = (start + k) % n;
            if Some(victim) == me {
                continue;
            }
            if let Some(job) = self.locals[victim].lock().unwrap().pop_front() {
                return Some(job);
            }
        }
        None
    }

    fn submit(&self, job: Job) {
        match WORKER_INDEX.with(|w| w.get()) {
            Some(i) => self.locals[i].lock().unwrap().push_back(job),
            None => self.injector.lock().unwrap().push_back(job),
        }
        self.wake.1.notify_all();
    }
}

/// Queues a closure on the work-stealing pool.
pub fn submit_job(job: impl FnOnce() + Send + 'static) {
    pool().submit(Box::new(job));
}

/// Result slot shared between a pooled job and whoever waits for it.
pub struct Slot {
    result: Mutex<Option<TaskOutcome>>,
    done: Condvar,
}

impl Slot {
    fn new() -> Arc<Slot> {
        Arc::new(Slot { result: Mutex::new(None), done: Condvar::new() })
    }

    fn complete(&self, outcome: TaskOutcome) {
        *self.result.lock().unwrap() = Some(outcome);
        self.done.notify_all();
    }

    pub fn is_done(&self) -> bool {
        self.result.lock().unwrap().is_some()
    }

    /// Waits for the result. Pool workers run other jobs while they wait ("helping").
    fn wait(&self) -> TaskOutcome {
        let me = WORKER_INDEX.with(|w| w.get());
        loop {
            if let Some(outcome) = self.result.lock().unwrap().take() {
                return outcome;
            }
            if me.is_some() {
                if let Some(job) = pool().find_job(me) {
                    run_job(job);
                    continue;
                }
            }
            let guard = self.result.lock().unwrap();
            if guard.is_some() {
                continue;
            }
            let _ = self.done.wait_timeout(guard, Duration::from_millis(2)).unwrap();
        }
    }
}

fn pool_tasks() -> &'static Mutex<HashMap<i64, Arc<Slot>>> {
    static POOL_TASKS: OnceLock<Mutex<HashMap<i64, Arc<Slot>>>> = OnceLock::new();
    POOL_TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn submit_task(program: Arc<Program>, name: String, args: Vec<Data>) -> Arc<Slot> {
    let slot = Slot::new();
    let done = slot.clone();
    submit_job(move || done.complete(execute_task(program, &name, args)));
    slot
}

/// Queues `name(args...)` on the work-stealing pool and returns a task id for `task_join`.
pub fn pool_spawn(program: Arc<Program>, name: String, args: Vec<Data>) -> i64 {
    let slot = submit_task(program, name, args);
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    pool_tasks().lock().unwrap().insert(id, slot);
    id
}

/// Waits for a task started with `spawn` or `pool_spawn`. A task can be joined only once.
pub fn join(id: i64) -> Result<(Data, Vec<String>), String> {
    let thread_handle = tasks().lock().unwrap().remove(&id);
    if let Some(handle) = thread_handle {
        return match handle.join() {
            Ok(outcome) => outcome.map_err(|e| format!("task {} failed: {}", id, e)),
            Err(payload) => Err(format!("task {} failed: {}", id, panic_message(payload))),
        };
    }
    let slot = pool_tasks().lock().unwrap().remove(&id);
    match slot {
        Some(slot) => slot.wait().map_err(|e| format!("task {} failed: {}", id, e)),
        None => Err(format!("task {} does not exist or was already joined", id)),
    }
}

pub fn is_finished(id: i64) -> Result<bool, String> {
    if let Some(h) = tasks().lock().unwrap().get(&id) {
        return Ok(h.is_finished());
    }
    if let Some(slot) = pool_tasks().lock().unwrap().get(&id) {
        return Ok(slot.is_done());
    }
    Err(format!("task {} does not exist or was already joined", id))
}

/// Applies `name` to every item on the work-stealing pool; results keep input order.
/// Items are processed in chunks (several chunks per worker) so that idle workers can steal.
pub fn parallel_map(program: &Arc<Program>, name: &str, items: Vec<Data>) -> Result<(Vec<Data>, Vec<String>), String> {
    let n = items.len();
    if n == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let workers = cpu_count().max(2);
    let chunk_size = ((n + workers * 4 - 1) / (workers * 4)).max(1);
    let mut slots = Vec::new();
    let mut remaining = items;
    while !remaining.is_empty() {
        let rest = remaining.split_off(chunk_size.min(remaining.len()));
        let chunk = std::mem::replace(&mut remaining, rest);
        let slot = Slot::new();
        let done = slot.clone();
        let program = program.clone();
        let name = name.to_string();
        submit_job(move || {
            let mut interp = Interpreter::new();
            interp.attach_program(program.clone());
            if catch_unwind(AssertUnwindSafe(|| interp.register_declarations(&program))).is_err() {
                done.complete(Err("could not initialise task interpreter".to_string()));
                return;
            }
            let mut results = Vec::with_capacity(chunk.len());
            let mut lines = Vec::new();
            for item in chunk {
                match run_function(&mut interp, &name, vec![item]) {
                    Ok((data, out)) => {
                        results.push(data);
                        lines.extend(out);
                    }
                    Err(e) => {
                        done.complete(Err(e));
                        return;
                    }
                }
            }
            done.complete(Ok((Data::List(results), lines)));
        });
        slots.push(slot);
    }
    let mut values = Vec::with_capacity(n);
    let mut output = Vec::new();
    for slot in slots {
        match slot.wait() {
            Ok((Data::List(chunk), lines)) => {
                values.extend(chunk);
                output.extend(lines);
            }
            Ok(_) => return Err("parallel_map produced an unexpected result".to_string()),
            Err(e) => return Err(format!("parallel_map: {}", e)),
        }
    }
    Ok((values, output))
}

// ---------------------------------------------------------------------------
// Structured concurrency: scopes
// ---------------------------------------------------------------------------
//
// A scope owns the tasks spawned into it. `scope_join` waits for *all* of them before it
// returns, so no child outlives the code that created the scope. If any child fails the scope is
// marked cancelled (siblings can poll `scope_is_cancelled` and stop early), the remaining
// children are still awaited, and the first error is reported. Cancellation is cooperative.

struct Scope {
    children: Mutex<Vec<Arc<Slot>>>,
    cancelled: AtomicBool,
}

fn scopes() -> &'static Mutex<HashMap<i64, Arc<Scope>>> {
    static SCOPES: OnceLock<Mutex<HashMap<i64, Arc<Scope>>>> = OnceLock::new();
    SCOPES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn scope(id: i64) -> Result<Arc<Scope>, String> {
    scopes().lock().unwrap().get(&id).cloned().ok_or_else(|| format!("scope {} does not exist or was already joined", id))
}

pub fn scope_new() -> i64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    scopes().lock().unwrap().insert(id, Arc::new(Scope { children: Mutex::new(Vec::new()), cancelled: AtomicBool::new(false) }));
    id
}

/// Spawns a child task into the scope; returns the child's index (its position in `scope_join`'s result).
pub fn scope_spawn(program: Arc<Program>, id: i64, name: String, args: Vec<Data>) -> Result<i64, String> {
    let sc = scope(id)?;
    if sc.cancelled.load(Ordering::SeqCst) {
        return Err(format!("scope {} is cancelled", id));
    }
    let slot = submit_task(program, name, args);
    let mut children = sc.children.lock().unwrap();
    children.push(slot);
    Ok(children.len() as i64 - 1)
}

pub fn scope_cancel(id: i64) -> Result<(), String> {
    scope(id)?.cancelled.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn scope_is_cancelled(id: i64) -> Result<bool, String> {
    Ok(scope(id)?.cancelled.load(Ordering::SeqCst))
}

/// Waits for every child, then returns their results in spawn order (or the first error).
pub fn scope_join(id: i64) -> Result<(Vec<Data>, Vec<String>), String> {
    let sc = scopes().lock().unwrap().remove(&id).ok_or_else(|| format!("scope {} does not exist or was already joined", id))?;
    let children: Vec<Arc<Slot>> = std::mem::take(&mut *sc.children.lock().unwrap());
    let mut values = Vec::new();
    let mut output = Vec::new();
    let mut first_error: Option<String> = None;
    for (index, slot) in children.into_iter().enumerate() {
        match slot.wait() {
            Ok((data, lines)) => {
                values.push(data);
                output.extend(lines);
            }
            Err(e) => {
                sc.cancelled.store(true, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(format!("scope child {} failed: {}", index, e));
                }
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok((values, output)),
    }
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

struct ChannelState {
    items: VecDeque<Data>,
    closed: bool,
}

struct Channel {
    state: Mutex<ChannelState>,
    changed: Condvar,
    capacity: usize,
}

fn channels() -> &'static Mutex<HashMap<i64, Arc<Channel>>> {
    static CHANNELS: OnceLock<Mutex<HashMap<i64, Arc<Channel>>>> = OnceLock::new();
    CHANNELS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn channel(id: i64) -> Result<Arc<Channel>, String> {
    channels().lock().unwrap().get(&id).cloned().ok_or_else(|| format!("channel {} does not exist", id))
}

/// Creates a channel. `capacity == 0` means unbounded; otherwise `send` blocks while full.
pub fn channel_new(capacity: i64) -> i64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let ch = Channel {
        state: Mutex::new(ChannelState { items: VecDeque::new(), closed: false }),
        changed: Condvar::new(),
        capacity: capacity.max(0) as usize,
    };
    channels().lock().unwrap().insert(id, Arc::new(ch));
    id
}

pub fn channel_send(id: i64, data: Data) -> Result<(), String> {
    let ch = channel(id)?;
    let mut st = ch.state.lock().unwrap();
    loop {
        if st.closed {
            return Err(format!("channel {} is closed", id));
        }
        if ch.capacity == 0 || st.items.len() < ch.capacity {
            break;
        }
        st = ch.changed.wait(st).unwrap();
    }
    st.items.push_back(data);
    ch.changed.notify_all();
    Ok(())
}

/// Receives one message. Returns `None` when the channel is closed and drained, or when the
/// timeout elapses (`timeout = None` waits forever).
pub fn channel_recv(id: i64, timeout: Option<Duration>) -> Result<Option<Data>, String> {
    let ch = channel(id)?;
    let deadline = timeout.map(|t| Instant::now() + t);
    let mut st = ch.state.lock().unwrap();
    loop {
        if let Some(item) = st.items.pop_front() {
            ch.changed.notify_all();
            return Ok(Some(item));
        }
        if st.closed {
            return Ok(None);
        }
        match deadline {
            None => st = ch.changed.wait(st).unwrap(),
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    return Ok(None);
                }
                let (guard, _) = ch.changed.wait_timeout(st, d - now).unwrap();
                st = guard;
            }
        }
    }
}

pub fn channel_try_recv(id: i64) -> Result<Option<Data>, String> {
    let ch = channel(id)?;
    let mut st = ch.state.lock().unwrap();
    let item = st.items.pop_front();
    if item.is_some() {
        ch.changed.notify_all();
    }
    Ok(item)
}

pub fn channel_len(id: i64) -> Result<i64, String> {
    Ok(channel(id)?.state.lock().unwrap().items.len() as i64)
}

/// Closes a channel: pending messages can still be received, new sends fail.
pub fn channel_close(id: i64) -> Result<(), String> {
    let ch = channel(id)?;
    ch.state.lock().unwrap().closed = true;
    ch.changed.notify_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Atomic counters
// ---------------------------------------------------------------------------

fn counters() -> &'static Mutex<HashMap<i64, Arc<AtomicI64>>> {
    static COUNTERS: OnceLock<Mutex<HashMap<i64, Arc<AtomicI64>>>> = OnceLock::new();
    COUNTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn counter(id: i64) -> Result<Arc<AtomicI64>, String> {
    counters().lock().unwrap().get(&id).cloned().ok_or_else(|| format!("counter {} does not exist", id))
}

pub fn counter_new(initial: i64) -> i64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    counters().lock().unwrap().insert(id, Arc::new(AtomicI64::new(initial)));
    id
}

pub fn counter_add(id: i64, delta: i64) -> Result<i64, String> {
    Ok(counter(id)?.fetch_add(delta, Ordering::SeqCst).wrapping_add(delta))
}

pub fn counter_get(id: i64) -> Result<i64, String> {
    Ok(counter(id)?.load(Ordering::SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_fifo_and_close() {
        let ch = channel_new(0);
        channel_send(ch, Data::Int(1)).unwrap();
        channel_send(ch, Data::Str("two".into())).unwrap();
        assert_eq!(channel_len(ch).unwrap(), 2);
        assert_eq!(channel_recv(ch, None).unwrap(), Some(Data::Int(1)));
        channel_close(ch).unwrap();
        assert!(channel_send(ch, Data::Null).is_err());
        assert_eq!(channel_recv(ch, None).unwrap(), Some(Data::Str("two".into())));
        assert_eq!(channel_recv(ch, None).unwrap(), None);
    }

    #[test]
    fn channel_timeout_and_cross_thread() {
        let ch = channel_new(1);
        assert_eq!(channel_recv(ch, Some(Duration::from_millis(20))).unwrap(), None);
        let producer = thread::spawn(move || {
            for i in 0..5 {
                channel_send(ch, Data::Int(i)).unwrap();
            }
            channel_close(ch).unwrap();
        });
        let mut got = Vec::new();
        while let Some(Data::Int(i)) = channel_recv(ch, None).unwrap() {
            got.push(i);
        }
        producer.join().unwrap();
        assert_eq!(got, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn counters_are_atomic() {
        let c = counter_new(0);
        let threads: Vec<_> = (0..8).map(|_| thread::spawn(move || {
            for _ in 0..1000 {
                counter_add(c, 1).unwrap();
            }
        })).collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(counter_get(c).unwrap(), 8000);
    }
}

#[cfg(test)]
mod pool_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn pool_runs_every_job() {
        let counter = Arc::new(AtomicUsize::new(0));
        let slots: Vec<Arc<Slot>> = (0..200)
            .map(|_| {
                let slot = Slot::new();
                let (done, counter) = (slot.clone(), counter.clone());
                submit_job(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    done.complete(Ok((Data::Null, Vec::new())));
                });
                slot
            })
            .collect();
        for slot in slots {
            assert!(slot.wait().is_ok());
        }
        assert_eq!(counter.load(Ordering::SeqCst), 200);
    }

    #[test]
    fn nested_fork_join_does_not_exhaust_the_pool() {
        // Each outer job waits for inner jobs; with more outer jobs than workers this deadlocks
        // unless waiting workers help by running queued jobs.
        let outer_count = cpu_count() * 4;
        let total = Arc::new(AtomicUsize::new(0));
        let outers: Vec<Arc<Slot>> = (0..outer_count)
            .map(|_| {
                let slot = Slot::new();
                let (done, total) = (slot.clone(), total.clone());
                submit_job(move || {
                    let inners: Vec<Arc<Slot>> = (0..8)
                        .map(|_| {
                            let inner = Slot::new();
                            let (inner_done, total) = (inner.clone(), total.clone());
                            submit_job(move || {
                                total.fetch_add(1, Ordering::SeqCst);
                                inner_done.complete(Ok((Data::Null, Vec::new())));
                            });
                            inner
                        })
                        .collect();
                    for inner in inners {
                        let _ = inner.wait();
                    }
                    done.complete(Ok((Data::Null, Vec::new())));
                });
                slot
            })
            .collect();
        for slot in outers {
            assert!(slot.wait().is_ok());
        }
        assert_eq!(total.load(Ordering::SeqCst), outer_count * 8);
    }

    #[test]
    fn idle_workers_steal_from_a_busy_workers_deque() {
        // One job queues many children on its own deque and then keeps its worker busy;
        // the children can only finish promptly if other workers steal them.
        let finished = Arc::new(AtomicUsize::new(0));
        let parent = Slot::new();
        let (done, fin) = (parent.clone(), finished.clone());
        submit_job(move || {
            for _ in 0..16 {
                let fin = fin.clone();
                submit_job(move || {
                    fin.fetch_add(1, Ordering::SeqCst);
                });
            }
            let start = Instant::now();
            while fin.load(Ordering::SeqCst) < 16 && start.elapsed() < Duration::from_secs(5) {
                thread::sleep(Duration::from_millis(1));
            }
            done.complete(Ok((Data::Int(fin.load(Ordering::SeqCst) as i64), Vec::new())));
        });
        let (result, _) = parent.wait().unwrap();
        assert_eq!(result, Data::Int(16));
    }
}
