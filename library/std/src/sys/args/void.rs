//! argv VOID: ядро отдаёт NUL-разделённый блоб по SYS_ARGS(0); [0] — имя программы.

pub use super::common::Args;
use crate::ffi::OsString;
use crate::sys::pal::abi;

pub fn args() -> Args {
    // ARGS_MAX ядра — 512 Б на доп. аргументы; имя короче корня store.
    let mut buf = [0u8; 1024];
    let total = abi::sys_args(0, &mut buf);
    let n = total.min(buf.len());
    let mut out = Vec::new();
    for a in buf[..n].split(|&b| b == 0).filter(|s| !s.is_empty()) {
        out.push(OsString::from(String::from_utf8_lossy(a).into_owned()));
    }
    Args::new(out)
}
