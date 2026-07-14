//! Платформенный слой VOID — микроядро с персистентным контент-адресуемым
//! объектным store. Процесс = статический ELF, загруженный ядром по content-id;
//! точка входа `_start` (регистры a0/a1 несут стартовые capability — legacy-путь,
//! std их не использует: argv/env спрашиваются у ядра через SYS_ARGS).
#![allow(unsafe_op_in_unsafe_fn)]

pub mod abi;

use crate::io;

/// Точка входа процесса (`e_entry` ELF). Стек уже выставлен ядром.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start(_a0: usize, _a1: usize) -> ! {
    unsafe extern "C" {
        fn main(_: isize, _: *const *const u8) -> i32;
    }
    let code = unsafe { main(0, core::ptr::null()) };
    abi::sys_exit(code as usize)
}

// SAFETY: вызывается ровно один раз при инициализации рантайма (из lang_start).
pub unsafe fn init(_argc: isize, _argv: *const *const u8, _sigpipe: u8) {}

// SAFETY: вызывается ровно один раз при завершении рантайма (не гарантируется).
pub unsafe fn cleanup() {}

pub fn unsupported<T>() -> io::Result<T> {
    Err(unsupported_err())
}

pub fn unsupported_err() -> io::Error {
    io::Error::UNSUPPORTED_PLATFORM
}

pub fn abort_internal() -> ! {
    abi::sys_exit(101)
}
