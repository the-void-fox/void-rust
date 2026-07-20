//! Нити VOID (Веха 35): ядро планирует НИТИ внутри процесса (общее адресное пространство и
//! домен, свой стек/TLS) через `SYS_THREAD_SPAWN/EXIT/JOIN`. `Thread::new` выделяет стек из
//! кучи процесса, боксит `ThreadInit` и запускает трамплин; тот СНАЧАЛА строит TLS нити
//! (иначе `thread::current` в `init()` тронул бы неготовый `#[thread_local]`), затем исполняет
//! замыкание и завершает нить. sleep/yield — как прежде (монотонное время U-mode).

use super::super::pal::{abi, tls};
use crate::ffi::CStr;
use crate::num::NonZero;
use crate::io;
use crate::thread::ThreadInit;
use crate::time::{Duration, Instant};

/// Стек нити по умолчанию (когда `Builder` не задал свой): скромный к 16-МиБ арене pal.
pub const DEFAULT_MIN_STACK_SIZE: usize = 256 * 1024;

pub struct Thread {
    tid: usize,
    /// Стек нити (выделен из кучи процесса) — освобождается в `join`, когда нить уже сошла с него.
    stack: *mut u8,
    stack_layout: crate::alloc::Layout,
}

// Дескриптор нити безопасно передавать/делить между нитями: это лишь id + указатель на её стек.
unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

/// Трамплин дочерней нити: `arg` — сырой `Box<ThreadInit>`. TLS — ПЕРВЫМ делом.
extern "C" fn thread_start(arg: usize) -> ! {
    // SAFETY: на новой нити TLS ещё не построен — строим до любого #[thread_local].
    unsafe { tls::install() };
    // SAFETY: `arg` — Box::into_raw(init).expose_provenance() из Thread::new; материализуем
    // указатель обратно и перекладываем владение (strict provenance std).
    let init = unsafe { Box::from_raw(crate::ptr::with_exposed_provenance_mut::<ThreadInit>(arg)) };
    let rust_start = init.init(); // ставит thread::current (нужен TLS) и возвращает замыкание
    rust_start();
    abi::sys_thread_exit(0);
}

impl Thread {
    // unsafe: см. thread::Builder::spawn_unchecked.
    pub unsafe fn new(stack: usize, init: Box<ThreadInit>) -> io::Result<Thread> {
        let stack_size = stack.max(DEFAULT_MIN_STACK_SIZE);
        let layout = crate::alloc::Layout::from_size_align(stack_size, 16).unwrap();
        // SAFETY: layout ненулевой (stack_size ≥ DEFAULT_MIN_STACK_SIZE).
        let stack_mem = unsafe { crate::alloc::alloc(layout) };
        if stack_mem.is_null() {
            return Err(io::const_error!(io::ErrorKind::OutOfMemory, "не выделить стек нити"));
        }
        let stack_top = (stack_mem.addr() + stack_size) & !0xf; // вершина вниз до 16
        let arg = Box::into_raw(init).expose_provenance();
        // Через тип fn-указателя (прямой каст fn-элемента в int ловит lint).
        let entry = thread_start as extern "C" fn(usize) -> !;
        let tid = abi::sys_thread_spawn(entry as usize, arg, stack_top);
        if tid == usize::MAX {
            // Ядро отклонило — вернуть Box (нить его не забрала) и стек.
            drop(unsafe { Box::from_raw(crate::ptr::with_exposed_provenance_mut::<ThreadInit>(arg)) });
            unsafe { crate::alloc::dealloc(stack_mem, layout) };
            return Err(io::const_error!(io::ErrorKind::Uncategorized, "ядро отклонило spawn нити"));
        }
        Ok(Thread { tid, stack: stack_mem, stack_layout: layout })
    }

    pub fn join(self) {
        abi::sys_thread_join(self.tid);
        // Нить завершилась и сошла со стека — можно освободить. TLS-блок нити утекает
        // (деструкторов TLS у нас нет: guard::enable — no-op).
        unsafe { crate::alloc::dealloc(self.stack, self.stack_layout) };
    }
}

pub fn available_parallelism() -> io::Result<NonZero<usize>> {
    // Один CPU (SMP нет) — но нити реальные и вытесняющие.
    Ok(NonZero::new(1).unwrap())
}

pub fn current_os_id() -> Option<u64> {
    // id нити ядру известен, но нить не спрашивает его о себе — std это не обязательно.
    None
}

pub fn set_name(_name: &CStr) {
    // Имён нитей ядро не ведёт — no-op.
}

pub fn yield_now() {
    abi::sys_yield();
}

pub fn sleep(dur: Duration) {
    let deadline = Instant::now().checked_add(dur);
    match deadline {
        Some(deadline) => sleep_until(deadline),
        None => loop {
            yield_now();
        },
    }
}

pub fn sleep_until(deadline: Instant) {
    while Instant::now() < deadline {
        yield_now();
    }
}
