//! Пути VOID: персоналия v1 плоская — рабочий каталог всегда корень.

use crate::io;
use crate::path::{Path, PathBuf};
use crate::sys::unsupported;

pub fn getcwd() -> io::Result<PathBuf> {
    Ok(PathBuf::from("/"))
}

pub fn chdir(p: &Path) -> io::Result<()> {
    // Уйти можно только «в корень» — где мы и так есть.
    match p.to_str() {
        Some("/") | Some(".") | Some("./") => Ok(()),
        _ => unsupported(),
    }
}

pub fn temp_dir() -> PathBuf {
    PathBuf::from("/")
}
