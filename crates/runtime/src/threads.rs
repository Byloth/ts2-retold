//! Threads and the objects they wait on.
//!
//! The program's threads run one at a time and take turns at points fixed by
//! the number of instructions executed, never by the host's clock or
//! scheduler, so that a run repeats exactly. A thread gives up its turn when
//! it has to wait or when its time slice ends.
//!
//! A thread that must wait while it is inside a callback (a window procedure,
//! say) cannot be set aside, because part of its state is on the host's
//! stack. The other threads then run "on top" of it until its wait is over.

use andy_cpu::interp::Cpu;

use crate::{Machine, Stop};

/// Instructions a thread runs before another gets a turn: one millisecond of
/// the virtual clock.
pub(crate) const TIME_SLICE: u64 = crate::INSTRUCTIONS_PER_MS;
const FIRST_HANDLE: u32 = 0x7000_0000;
const THREAD_STACK_SIZE: u32 = 0x0010_0000;
/// Each thread gets a megabyte of stack below this address, then a guard gap.
const THREAD_STACKS_TOP: u32 = 0x0F00_0000;
const THREAD_BLOCKS: u32 = 0x7FFA_0000;

pub const WAIT_OBJECT_0: u32 = 0;
pub const WAIT_TIMEOUT: u32 = 0x102;
pub const WAIT_FAILED: u32 = u32::MAX;
pub const INFINITE: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    Event {
        manual_reset: bool,
        signaled: bool,
    },
    Semaphore {
        count: u32,
        maximum: u32,
    },
    /// Signaled once the thread has finished.
    Thread(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Waiting,
    Finished,
}

/// What a waiting thread waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Waiting {
    /// Any one, or all, of some objects.
    Objects { handles: Vec<u32>, all: bool },
    /// A critical section, identified by its address.
    CriticalSection(u32),
    /// Nothing but the passing of time.
    Time,
}

#[derive(Debug, Clone)]
struct Wait {
    what: Waiting,
    /// Clock value at which the wait gives up.
    deadline_ms: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct Thread {
    /// Register state while the thread is not the running one.
    cpu: Cpu,
    callback_depth: u32,
    state: State,
    wait: Option<Wait>,
}

#[derive(Debug, Clone, Copy, Default)]
struct CriticalSection {
    owner: Option<usize>,
    depth: u32,
}

#[derive(Debug, Default)]
pub struct Threads {
    threads: Vec<Thread>,
    /// Index of the thread whose registers are in `Machine::cpu`.
    current: usize,
    objects: Vec<Option<Object>>,
    critical_sections: std::collections::HashMap<u32, CriticalSection>,
    /// Milliseconds the clock was moved forward while every thread waited.
    pub(crate) skipped_ms: u64,
    /// Instruction count at which the running thread's turn ends.
    pub(crate) slice_end: u64,
    /// Threads waiting inside a callback, innermost last. Their state is
    /// partly on the host's stack, so only the innermost can be resumed.
    nested_waiters: Vec<usize>,
}

impl Threads {
    pub(crate) fn new() -> Threads {
        let main = Thread { cpu: Cpu::default(), callback_depth: 0, state: State::Ready, wait: None };
        Threads { threads: vec![main], ..Threads::default() }
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn count(&self) -> usize {
        self.threads.len()
    }

    pub(crate) fn create_object(&mut self, object: Object) -> u32 {
        let index = self.objects.iter().position(Option::is_none).unwrap_or_else(|| {
            self.objects.push(None);
            self.objects.len() - 1
        });
        self.objects[index] = Some(object);
        FIRST_HANDLE + 4 * index as u32
    }

    pub(crate) fn object(&mut self, handle: u32) -> Option<&mut Object> {
        let offset = handle.checked_sub(FIRST_HANDLE)?;
        self.objects.get_mut((offset / 4) as usize).filter(|_| offset % 4 == 0)?.as_mut()
    }

    /// Closes a handle; false if it is not one of ours.
    pub(crate) fn close(&mut self, handle: u32) -> bool {
        match handle.checked_sub(FIRST_HANDLE).map(|offset| (offset / 4) as usize) {
            Some(index) if index < self.objects.len() => self.objects[index].take().is_some(),
            _ => false,
        }
    }

    fn is_signaled(&self, handle: u32) -> Option<bool> {
        let offset = handle.checked_sub(FIRST_HANDLE)?;
        Some(match self.objects.get((offset / 4) as usize)?.as_ref()? {
            Object::Event { signaled, .. } => *signaled,
            Object::Semaphore { count, .. } => *count > 0,
            Object::Thread(index) => self.threads[*index].state == State::Finished,
        })
    }

    /// Takes what a successful wait takes: an automatic event resets, a
    /// semaphore gives up one count.
    fn acquire(&mut self, handle: u32) {
        match self.object(handle) {
            Some(Object::Event { manual_reset: false, signaled }) => *signaled = false,
            Some(Object::Semaphore { count, .. }) => *count -= 1,
            _ => {}
        }
    }

    /// Whether a wait could complete now, without completing it.
    fn satisfied(&self, thread: usize, what: &Waiting) -> bool {
        match what {
            Waiting::Objects { handles, all: true } => {
                handles.iter().all(|&h| self.is_signaled(h) == Some(true))
            }
            Waiting::Objects { handles, all: false } => {
                handles.iter().any(|&h| self.is_signaled(h) == Some(true))
            }
            Waiting::CriticalSection(address) => self
                .critical_sections
                .get(address)
                .is_none_or(|cs| cs.owner.is_none_or(|owner| owner == thread)),
            Waiting::Time => false,
        }
    }

    /// Completes a wait if possible. Returns the value the wait call yields.
    fn try_complete(&mut self, thread: usize, what: &Waiting) -> Option<u32> {
        if !self.satisfied(thread, what) {
            return None;
        }
        match what {
            Waiting::Objects { handles, all: true } => {
                for &handle in handles {
                    self.acquire(handle);
                }
                Some(WAIT_OBJECT_0)
            }
            Waiting::Objects { handles, all: false } => {
                let first = handles.iter().position(|&h| self.is_signaled(h) == Some(true))?;
                self.acquire(handles[first]);
                Some(WAIT_OBJECT_0 + first as u32)
            }
            Waiting::CriticalSection(address) => {
                let section = self.critical_sections.entry(*address).or_default();
                section.owner = Some(thread);
                section.depth += 1;
                Some(0)
            }
            Waiting::Time => None,
        }
    }

    pub(crate) fn leave_critical_section(&mut self, address: u32) {
        if let Some(section) = self.critical_sections.get_mut(&address)
            && section.owner == Some(self.current)
        {
            section.depth -= 1;
            if section.depth == 0 {
                section.owner = None;
            }
        }
    }
}

/// Why execution of the current thread paused without stopping the machine.
pub(crate) enum Pause {
    /// The host called into the program and that call returned.
    CallbackReturned,
    /// Another thread should run now.
    Yield,
}

impl Machine {
    /// The virtual clock, in milliseconds since the machine started.
    pub(crate) fn clock_ms(&self) -> u64 {
        self.cpu.instructions / crate::INSTRUCTIONS_PER_MS + self.threads.skipped_ms
    }

    /// Starts a thread of the program at `entry` with one argument.
    pub(crate) fn create_thread(&mut self, entry: u32, argument: u32) -> u32 {
        let index = self.threads.threads.len() as u32;
        let stack_top = THREAD_STACKS_TOP - index * 2 * THREAD_STACK_SIZE;
        let block = THREAD_BLOCKS + index * 0x1000;
        self.memory.map(stack_top - THREAD_STACK_SIZE, THREAD_STACK_SIZE);
        self.memory.map(block, 0x1000);
        for (offset, value) in
            [(0x00, u32::MAX), (0x04, stack_top), (0x08, stack_top - THREAD_STACK_SIZE), (0x18, block)]
        {
            self.memory.write(block + offset, 4, value).expect("thread block was just mapped");
        }
        let mut cpu = Cpu { eip: entry, fs_base: block, ..Cpu::default() };
        cpu.fpu.control = self.cpu.fpu.control;
        cpu.regs[4] = stack_top - 0x100;
        self.memory.write(cpu.regs[4], 4, crate::THREAD_EXIT).expect("stack was just mapped");
        self.memory.write(cpu.regs[4] + 4, 4, argument).expect("stack was just mapped");
        self.threads.threads.push(Thread { cpu, callback_depth: 0, state: State::Ready, wait: None });
        self.threads.create_object(Object::Thread(index as usize))
    }

    /// Marks the running thread finished; another one runs next.
    pub(crate) fn finish_current_thread(&mut self) {
        self.threads.threads[self.threads.current].state = State::Finished;
    }

    fn switch_to(&mut self, next: usize) {
        let current = self.threads.current;
        if next != current {
            // The instruction counter is the machine's clock: it carries over.
            let instructions = self.cpu.instructions;
            let outgoing = &mut self.threads.threads[current];
            std::mem::swap(&mut outgoing.cpu, &mut self.cpu);
            outgoing.callback_depth = self.callback_depth;
            let incoming = &mut self.threads.threads[next];
            std::mem::swap(&mut incoming.cpu, &mut self.cpu);
            self.callback_depth = incoming.callback_depth;
            self.cpu.instructions = instructions;
            self.threads.current = next;
        }
        self.threads.slice_end = self.cpu.instructions + TIME_SLICE;
    }

    /// Wakes the threads whose wait is over or has timed out.
    fn wake_threads(&mut self) {
        let now = self.clock_ms();
        for index in 0..self.threads.threads.len() {
            let thread = &self.threads.threads[index];
            if thread.state != State::Waiting {
                continue;
            }
            let wait = thread.wait.as_ref().expect("a waiting thread has a wait");
            let timed_out = wait.deadline_ms.is_some_and(|deadline| now >= deadline);
            if timed_out || self.threads.satisfied(index, &wait.what) {
                self.threads.threads[index].state = State::Ready;
            }
        }
    }

    /// Picks the thread that runs next and makes it current. If every thread
    /// is waiting, the clock jumps to the first timeout.
    fn schedule(&mut self) -> Result<(), Stop> {
        loop {
            self.wake_threads();
            let count = self.threads.threads.len();
            let innermost = self.threads.nested_waiters.last().copied();
            let resumable =
                |index: usize| Some(index) == innermost || !self.threads.nested_waiters.contains(&index);
            let next = (1..=count)
                .map(|step| (self.threads.current + step) % count)
                .find(|&index| resumable(index) && self.threads.threads[index].state == State::Ready);
            if let Some(next) = next {
                self.switch_to(next);
                return Ok(());
            }
            let earliest = self
                .threads
                .threads
                .iter()
                .filter(|thread| thread.state == State::Waiting)
                .filter_map(|thread| thread.wait.as_ref()?.deadline_ms)
                .min();
            match earliest {
                Some(deadline) => {
                    let now = self.clock_ms();
                    self.threads.skipped_ms += deadline.saturating_sub(now).max(1);
                }
                None => return Err(Stop::Deadlock),
            }
        }
    }

    /// Runs the program's threads until the machine stops.
    pub(crate) fn run_threads(&mut self) -> Stop {
        self.threads.slice_end = self.cpu.instructions + TIME_SLICE;
        loop {
            match self.execute(false) {
                Err(stop) => return stop,
                Ok(Pause::CallbackReturned) => unreachable!("no callback is in progress at this level"),
                Ok(Pause::Yield) => {
                    if let Err(stop) = self.schedule() {
                        return stop;
                    }
                }
            }
        }
    }

    /// Waits, on behalf of a host function, for `what` or for `timeout_ms`.
    ///
    /// Returns the value the wait yields. `Err(Stop::WouldBlock)` means the
    /// thread has been put to sleep and the host function must simply
    /// propagate the error: it will be called again when the wait is over.
    pub(crate) fn wait(&mut self, what: Waiting, timeout_ms: u32) -> Result<u32, Stop> {
        let current = self.threads.current;
        let now = self.clock_ms();
        // A wait that was already begun keeps its original deadline.
        let deadline = match self.threads.threads[current].wait.take() {
            Some(wait) if wait.what == what => wait.deadline_ms,
            _ => (timeout_ms != INFINITE).then(|| now + timeout_ms as u64),
        };
        loop {
            if let Some(result) = self.threads.try_complete(current, &what) {
                return Ok(result);
            }
            if deadline.is_some_and(|deadline| self.clock_ms() >= deadline) {
                return Ok(WAIT_TIMEOUT);
            }
            let thread = &mut self.threads.threads[current];
            thread.wait = Some(Wait { what: what.clone(), deadline_ms: deadline });
            thread.state = State::Waiting;
            if self.callback_depth == 0 {
                return Err(Stop::WouldBlock);
            }
            // Inside a callback: let the other threads run on top of this
            // one until it can continue.
            self.threads.nested_waiters.push(current);
            let outcome = self.run_others_until_resumed(current);
            self.threads.nested_waiters.pop();
            outcome?;
            self.threads.threads[current].wait = None;
        }
    }

    fn run_others_until_resumed(&mut self, waiter: usize) -> Result<(), Stop> {
        loop {
            self.schedule()?;
            if self.threads.current == waiter {
                return Ok(());
            }
            match self.execute(false)? {
                Pause::Yield => {}
                Pause::CallbackReturned => unreachable!("the thread on top runs at its own base level"),
            }
        }
    }

    /// Gives up the rest of the running thread's turn.
    pub(crate) fn yield_slice(&mut self) {
        self.threads.slice_end = self.cpu.instructions;
    }
}
