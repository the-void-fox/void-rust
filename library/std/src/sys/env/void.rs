//! Окружение VOID: блоб `KEY=VAL\0…` по SYS_ARGS(1), унаследованный от родителя
//! при exec. Изменяемого окружения у процесса нет — setenv/unsetenv честно
//! отказывают (окружение задаёт родитель при запуске).

pub use super::common::Env;
use crate::ffi::{OsStr, OsString};
use crate::io;
use crate::sys::pal::abi;

fn snapshot() -> Vec<(OsString, OsString)> {
    let mut buf = [0u8; 1024];
    let total = abi::sys_args(1, &mut buf);
    let n = total.min(buf.len());
    let mut out = Vec::new();
    for entry in buf[..n].split(|&b| b == 0).filter(|s| !s.is_empty()) {
        let entry = String::from_utf8_lossy(entry);
        if let Some((k, v)) = entry.split_once('=') {
            out.push((OsString::from(k), OsString::from(v)));
        }
    }
    out
}

pub fn env() -> Env {
    Env::new(snapshot())
}

pub fn getenv(key: &OsStr) -> Option<OsString> {
    snapshot().into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub unsafe fn setenv(_key: &OsStr, _val: &OsStr) -> io::Result<()> {
    Err(io::Error::UNSUPPORTED_PLATFORM)
}

pub unsafe fn unsetenv(_key: &OsStr) -> io::Result<()> {
    Err(io::Error::UNSUPPORTED_PLATFORM)
}
