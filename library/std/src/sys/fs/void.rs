//! std::fs VOID (Веха 32): файлы — по IPC к посикс-персоналии `posixfs`.
//!
//! Эндпоинт сервера приходит процессу стартовой capability (слот 0 таблицы
//! наследования, Веха 30) — та самая «preopen»-модель WASI: у кого нет права,
//! у того нет и файловой системы. Протокол — зеркало `void_user::posix`
//! (`Code/programs/user/src/lib.rs`): op = opcode | fd<<8 | mode<<16,
//! нагрузка и ответ ≤ 512 байт за вызов.
//!
//! Персоналия v1 плоская: один корневой каталог, файл ≤ 4 КиБ, имя ≤ 32 байт
//! (слэши внутри имени легальны — `bin/riscv64/hello` это ИМЯ корня store).
//! Каталогов, симлинков, прав и времён нет — соответствующие операции честно
//! возвращают unsupported. `close` у сервера — атомарный коммит файла в store.
//!
//! `RawFd` (для `std::os::fd`, его ждёт uutils) — номер слота у сервера;
//! [`FileDesc`] владеет слотом и закрывает его в Drop. Конверсия через
//! `OwnedFd` теряет клиентское зеркало курсора (`pos` обнуляется) — честная
//! цена: fstat'а в протоколе нет, а редкая конверсия начинается с seek'а.

use crate::cell::Cell;
use crate::ffi::OsString;
use crate::fmt;
use crate::fs::TryLockError;
use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut, SeekFrom};
use crate::mem::ManuallyDrop;
use crate::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use crate::path::{Path, PathBuf};
use crate::string::String;
use crate::sys::pal::abi;
use crate::sys::time::SystemTime;
use crate::sys::{AsInner, AsInnerMut, FromInner, IntoInner, unsupported, unsupported_err};
use crate::vec::Vec;

#[expect(dead_code)]
#[path = "unsupported.rs"]
mod unsupported_fs;
pub use unsupported_fs::{Dir, DirBuilder, FileTimes, link, readlink, remove_dir_all, rmdir, symlink};

pub use crate::sys::fs::common::{copy, exists};

// ─── протокол posixfs (зеркало void_user::posix) ─────────────────────────────

const OP_OPEN: usize = 0;
const OP_READ: usize = 1;
const OP_WRITE: usize = 2;
const OP_CLOSE: usize = 3;
const OP_STAT: usize = 4;
const OP_UNLINK: usize = 5;
const OP_READDIR: usize = 6;
const OP_SEEK: usize = 7;
const OP_RENAME: usize = 8;

const O_APPEND: usize = 1 << 0;
const O_TRUNC: usize = 1 << 1;

const SEEK_SET: usize = 0;
const SEEK_CUR: usize = 1;
const SEEK_END: usize = 2;

/// Максимум нагрузки/ответа одного IPC-вызова (буферы сервера).
const MSG_MAX: usize = 512;
/// Предел размера файла в персоналии v1.
const DATA_MAX: usize = 4096;
/// Предел длины имени у сервера.
const NAME_MAX: usize = 32;

/// Эндпоинт персоналии — стартовая capability, слот 0 (наследуется от vsh).
fn fs_ep() -> io::Result<usize> {
    match abi::sys_start_cap(0) {
        abi::NO_CAP => Err(io::const_error!(
            io::ErrorKind::Unsupported,
            "нет capability файловой персоналии (стартовый слот 0 пуст)",
        )),
        ep => Ok(ep),
    }
}

fn err_ipc() -> io::Error {
    io::const_error!(io::ErrorKind::Uncategorized, "IPC к posixfs не удался")
}

/// Закрыть серверный слот (для `Drop` и `OwnedFd`): изменённый файл
/// коммитится в store (put + set_root). Ошибки close игнорируются, как везде.
pub fn close_raw(fd: RawFd) {
    if let Ok(ep) = fs_ep() {
        abi::sys_call(ep, OP_CLOSE | ((fd as usize) << 8), &[], &mut []);
    }
}

/// Имя корня store из пути: срезать ведущие `/` и `./`; пустота — это корневой
/// каталог (`None`). Плоское пространство: `..` не интерпретируется.
fn root_name(path: &Path) -> io::Result<Option<&str>> {
    let mut s = path
        .to_str()
        .ok_or(io::const_error!(io::ErrorKind::InvalidFilename, "имя — не UTF-8"))?;
    loop {
        if let Some(r) = s.strip_prefix('/') {
            s = r;
        } else if let Some(r) = s.strip_prefix("./") {
            s = r;
        } else {
            break;
        }
    }
    if s.is_empty() || s == "." {
        return Ok(None);
    }
    if s.len() > NAME_MAX {
        return Err(io::const_error!(
            io::ErrorKind::InvalidFilename,
            "имя длиннее 32 байт (предел посикс-персоналии v1)",
        ));
    }
    Ok(Some(s))
}

/// Как `root_name`, но каталог — это ошибка (для операций над файлами).
fn file_name(path: &Path) -> io::Result<&str> {
    root_name(path)?.ok_or(io::const_error!(
        io::ErrorKind::IsADirectory,
        "это корневой каталог, а не файл",
    ))
}

// ─── типы ────────────────────────────────────────────────────────────────────

/// Владеющий дескриптор слота posixfs: закрывает слот в Drop.
pub struct FileDesc {
    fd: RawFd,
    /// Зеркало серверного курсора: нужно, чтобы честно считать, сколько байт
    /// примет запись (сервер молча зажимает на границе 4 КиБ).
    pos: Cell<usize>,
}

// SAFETY: потоков в VOID нет (std::thread — unsupported); Cell не делится.
unsafe impl Send for FileDesc {}
unsafe impl Sync for FileDesc {}

impl Drop for FileDesc {
    fn drop(&mut self) {
        close_raw(self.fd);
    }
}

pub struct File(FileDesc);

#[derive(Clone)]
pub enum FileAttr {
    Dir,
    File { size: u64 },
}

pub struct ReadDir {
    names: crate::vec::IntoIter<String>,
}

pub struct DirEntry {
    name: String,
}

#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FilePermissions {}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileType {
    is_dir: bool,
}

impl FileAttr {
    pub fn size(&self) -> u64 {
        match self {
            Self::File { size } => *size,
            Self::Dir => 0,
        }
    }

    pub fn perm(&self) -> FilePermissions {
        FilePermissions {}
    }

    pub fn file_type(&self) -> FileType {
        FileType { is_dir: matches!(self, FileAttr::Dir) }
    }

    pub fn modified(&self) -> io::Result<SystemTime> {
        unsupported()
    }

    pub fn accessed(&self) -> io::Result<SystemTime> {
        unsupported()
    }

    pub fn created(&self) -> io::Result<SystemTime> {
        unsupported()
    }
}

impl FilePermissions {
    pub fn readonly(&self) -> bool {
        false
    }

    pub fn set_readonly(&mut self, _readonly: bool) {}
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub fn is_file(&self) -> bool {
        !self.is_dir
    }

    pub fn is_symlink(&self) -> bool {
        false // симлинков в персоналии v1 нет
    }
}

impl fmt::Debug for ReadDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadDir").finish_non_exhaustive()
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        self.names.next().map(|name| Ok(DirEntry { name }))
    }
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        let mut p = String::with_capacity(1 + self.name.len());
        p.push('/');
        p.push_str(&self.name);
        PathBuf::from(p)
    }

    pub fn file_name(&self) -> OsString {
        OsString::from(self.name.clone())
    }

    pub fn metadata(&self) -> io::Result<FileAttr> {
        stat(&self.path())
    }

    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(self.metadata()?.file_type())
    }
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
        }
    }

    pub fn read(&mut self, read: bool) {
        self.read = read;
    }
    pub fn write(&mut self, write: bool) {
        self.write = write;
    }
    pub fn append(&mut self, append: bool) {
        self.append = append;
    }
    pub fn truncate(&mut self, truncate: bool) {
        self.truncate = truncate;
    }
    pub fn create(&mut self, create: bool) {
        self.create = create;
    }
    pub fn create_new(&mut self, create_new: bool) {
        self.create_new = create_new;
    }
}

impl File {
    pub fn open(path: &Path, opts: &OpenOptions) -> io::Result<File> {
        let writing = opts.write || opts.append;
        if !opts.read && !writing {
            return Err(io::const_error!(io::ErrorKind::InvalidInput, "ни чтения, ни записи"));
        }
        if (opts.truncate || opts.create || opts.create_new) && !writing {
            return Err(io::const_error!(
                io::ErrorKind::InvalidInput,
                "truncate/create требуют записи",
            ));
        }
        let name = file_name(path)?;

        // Сервер создаёт файл при любом open — контракты `create_new`/`!create`
        // обеспечиваем сами предварительным stat'ом.
        let exists = matches!(stat(path), Ok(_));
        if opts.create_new && exists {
            return Err(io::const_error!(io::ErrorKind::AlreadyExists, "файл уже существует"));
        }
        if !opts.create && !opts.create_new && !exists {
            return Err(io::const_error!(io::ErrorKind::NotFound, "нет такого файла"));
        }

        let mut mode = 0usize;
        if opts.truncate {
            mode |= O_TRUNC;
        }
        if opts.append {
            mode |= O_APPEND;
        }

        let ep = fs_ep()?;
        let mut rep = [0u8; 1];
        let n = abi::sys_call(ep, OP_OPEN | (mode << 16), name.as_bytes(), &mut rep);
        if n != 1 {
            return Err(err_ipc());
        }
        if rep[0] == 0xff {
            return Err(io::const_error!(
                io::ErrorKind::Uncategorized,
                "нет свободных слотов у posixfs",
            ));
        }
        let file = File(FileDesc { fd: rep[0] as RawFd, pos: Cell::new(0) });
        if opts.append {
            // Сервер поставил курсор в конец — узнать его (и размер) честно.
            file.seek_ipc(0, SEEK_CUR)?;
        }
        Ok(file)
    }

    /// OP_SEEK: сместить серверный курсор, обновить зеркало. Возвращает новую
    /// позицию.
    fn seek_ipc(&self, off: i64, whence: usize) -> io::Result<u64> {
        let ep = fs_ep()?;
        let mut rep = [0u8; 8];
        let n = abi::sys_call(
            ep,
            OP_SEEK | ((self.0.fd as usize) << 8) | (whence << 16),
            &off.to_le_bytes(),
            &mut rep,
        );
        if n != 8 {
            return Err(err_ipc());
        }
        let pos = u64::from_le_bytes(rep);
        if pos == u64::MAX {
            return Err(io::const_error!(io::ErrorKind::InvalidInput, "seek отвергнут"));
        }
        self.0.pos.set(pos as usize);
        Ok(pos)
    }

    pub fn file_attr(&self) -> io::Result<FileAttr> {
        // fstat'а в протоколе нет — размер добывается парой seek'ов.
        let cur = self.seek_ipc(0, SEEK_CUR)?;
        let size = self.seek_ipc(0, SEEK_END)?;
        self.seek_ipc(cur as i64, SEEK_SET)?;
        Ok(FileAttr::File { size })
    }

    pub fn fsync(&self) -> io::Result<()> {
        Ok(()) // коммит в store делает close сервера
    }

    pub fn datasync(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn lock(&self) -> io::Result<()> {
        unsupported()
    }

    pub fn lock_shared(&self) -> io::Result<()> {
        unsupported()
    }

    pub fn try_lock(&self) -> Result<(), TryLockError> {
        Err(TryLockError::Error(unsupported_err()))
    }

    pub fn try_lock_shared(&self) -> Result<(), TryLockError> {
        Err(TryLockError::Error(unsupported_err()))
    }

    pub fn unlock(&self) -> io::Result<()> {
        unsupported()
    }

    pub fn truncate(&self, _size: u64) -> io::Result<()> {
        unsupported()
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let ep = fs_ep()?;
        let op = OP_READ | ((self.0.fd as usize) << 8);
        if buf.len() >= MSG_MAX {
            // Ответ сервера ≤ 512 байт — влезет целиком, курсоры согласованы.
            let n = abi::sys_call(ep, op, &[], buf);
            if n == usize::MAX {
                return Err(err_ipc());
            }
            self.0.pos.set(self.0.pos.get() + n);
            Ok(n)
        } else {
            // Маленький буфер клиента: сервер всё равно продвинет курсор на
            // размер СВОЕГО ответа — принять во временный и откатить излишек.
            let mut tmp = [0u8; MSG_MAX];
            let n = abi::sys_call(ep, op, &[], &mut tmp);
            if n == usize::MAX {
                return Err(err_ipc());
            }
            let take = n.min(buf.len());
            buf[..take].copy_from_slice(&tmp[..take]);
            if take < n {
                self.seek_ipc(-((n - take) as i64), SEEK_CUR)?;
            } else {
                self.0.pos.set(self.0.pos.get() + take);
            }
            Ok(take)
        }
    }

    pub fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        crate::io::default_read_vectored(|b| self.read(b), bufs)
    }

    #[inline]
    pub fn is_read_vectored(&self) -> bool {
        false
    }

    pub fn read_buf(&self, cursor: BorrowedCursor<'_>) -> io::Result<()> {
        crate::io::default_read_buf(|b| self.read(b), cursor)
    }

    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pos = self.0.pos.get();
        // Сервер молча зажимает запись на границе файла — зеркалим ту же
        // формулу и честно сообщаем «диск полон», когда места нет.
        let can = buf.len().min(MSG_MAX).min(DATA_MAX.saturating_sub(pos));
        if can == 0 {
            return Err(io::const_error!(
                io::ErrorKind::StorageFull,
                "файл в посикс-персоналии v1 ограничен 4 КиБ",
            ));
        }
        let ep = fs_ep()?;
        let n = abi::sys_call(ep, OP_WRITE | ((self.0.fd as usize) << 8), &buf[..can], &mut []);
        if n == usize::MAX {
            return Err(err_ipc());
        }
        self.0.pos.set(pos + can);
        Ok(can)
    }

    pub fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        crate::io::default_write_vectored(|b| self.write(b), bufs)
    }

    #[inline]
    pub fn is_write_vectored(&self) -> bool {
        false
    }

    pub fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn seek(&self, pos: SeekFrom) -> io::Result<u64> {
        let (off, whence) = match pos {
            SeekFrom::Start(o) => (
                i64::try_from(o).map_err(|_| {
                    io::const_error!(io::ErrorKind::InvalidInput, "смещение не влезает в i64")
                })?,
                SEEK_SET,
            ),
            SeekFrom::Current(o) => (o, SEEK_CUR),
            SeekFrom::End(o) => (o, SEEK_END),
        };
        self.seek_ipc(off, whence)
    }

    pub fn size(&self) -> Option<io::Result<u64>> {
        None
    }

    pub fn tell(&self) -> io::Result<u64> {
        Ok(self.0.pos.get() as u64)
    }

    pub fn duplicate(&self) -> io::Result<File> {
        unsupported()
    }

    pub fn set_permissions(&self, _perm: FilePermissions) -> io::Result<()> {
        // Прав в персоналии v1 нет — «установить права» тривиально выполнено
        // (иначе cp падал бы на финальном копировании прав).
        Ok(())
    }

    pub fn set_times(&self, _times: FileTimes) -> io::Result<()> {
        unsupported()
    }
}

impl fmt::Debug for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("File").field("fd", &self.0.fd).finish()
    }
}

// ─── интероп std::os::fd (нужен uutils: OwnedFd/AsFd в uucore) ───────────────

impl AsRawFd for FileDesc {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl IntoRawFd for FileDesc {
    fn into_raw_fd(self) -> RawFd {
        ManuallyDrop::new(self).fd // без close — владение ушло наружу
    }
}

impl IntoInner<OwnedFd> for FileDesc {
    fn into_inner(self) -> OwnedFd {
        // SAFETY: слот валиден (0..16), fd != -1; close теперь за OwnedFd.
        unsafe { OwnedFd::from_raw_fd(self.into_raw_fd()) }
    }
}

impl FromInner<OwnedFd> for FileDesc {
    fn from_inner(owned: OwnedFd) -> FileDesc {
        // Зеркало курсора обнуляется — реальную позицию знает только сервер.
        FileDesc { fd: owned.into_raw_fd(), pos: Cell::new(0) }
    }
}

impl AsInner<FileDesc> for File {
    #[inline]
    fn as_inner(&self) -> &FileDesc {
        &self.0
    }
}

impl AsInnerMut<FileDesc> for File {
    #[inline]
    fn as_inner_mut(&mut self) -> &mut FileDesc {
        &mut self.0
    }
}

impl IntoInner<FileDesc> for File {
    fn into_inner(self) -> FileDesc {
        self.0
    }
}

impl FromInner<FileDesc> for File {
    fn from_inner(fd: FileDesc) -> File {
        File(fd)
    }
}

impl AsRawFd for File {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        self.0.fd
    }
}

impl AsFd for File {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: слот открыт, пока жив File.
        unsafe { BorrowedFd::borrow_raw(self.0.fd) }
    }
}

// ─── операции над путями ─────────────────────────────────────────────────────

pub fn readdir(p: &Path) -> io::Result<ReadDir> {
    if root_name(p)?.is_some() {
        return Err(io::const_error!(
            io::ErrorKind::NotADirectory,
            "каталог тут один — корневой",
        ));
    }
    let ep = fs_ep()?;
    let mut rep = [0u8; MSG_MAX];
    let n = abi::sys_call(ep, OP_READDIR, &[], &mut rep);
    if n == usize::MAX {
        return Err(err_ipc());
    }
    let names: Vec<String> = String::from_utf8_lossy(&rep[..n])
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    Ok(ReadDir { names: names.into_iter() })
}

pub fn unlink(p: &Path) -> io::Result<()> {
    let name = file_name(p)?;
    stat(p)?; // честный NotFound: сервер сам не жалуется
    let ep = fs_ep()?;
    let mut rep = [0u8; 1];
    let n = abi::sys_call(ep, OP_UNLINK, name.as_bytes(), &mut rep);
    if n != 1 {
        return Err(err_ipc());
    }
    Ok(())
}

pub fn rename(old: &Path, new: &Path) -> io::Result<()> {
    let old = file_name(old)?;
    let new = file_name(new)?;
    let mut req = [0u8; 1 + NAME_MAX + NAME_MAX];
    req[0] = old.len() as u8;
    req[1..1 + old.len()].copy_from_slice(old.as_bytes());
    req[1 + old.len()..1 + old.len() + new.len()].copy_from_slice(new.as_bytes());
    let ep = fs_ep()?;
    let mut rep = [0u8; 1];
    let n = abi::sys_call(ep, OP_RENAME, &req[..1 + old.len() + new.len()], &mut rep);
    if n != 1 {
        return Err(err_ipc());
    }
    if rep[0] != 0 {
        return Err(io::const_error!(io::ErrorKind::NotFound, "нет такого файла"));
    }
    Ok(())
}

pub fn set_perm(_p: &Path, _perm: FilePermissions) -> io::Result<()> {
    Ok(()) // прав нет — см. File::set_permissions
}

pub fn set_times(_p: &Path, _times: FileTimes) -> io::Result<()> {
    unsupported()
}

pub fn set_times_nofollow(_p: &Path, _times: FileTimes) -> io::Result<()> {
    unsupported()
}

pub fn stat(p: &Path) -> io::Result<FileAttr> {
    match root_name(p)? {
        None => Ok(FileAttr::Dir),
        Some(name) => {
            let ep = fs_ep()?;
            let mut rep = [0u8; 5];
            let n = abi::sys_call(ep, OP_STAT, name.as_bytes(), &mut rep);
            if n != 5 {
                return Err(err_ipc());
            }
            if rep[0] == 0 {
                return Err(io::const_error!(io::ErrorKind::NotFound, "нет такого файла"));
            }
            let size = u32::from_le_bytes(rep[1..5].try_into().unwrap()) as u64;
            Ok(FileAttr::File { size })
        }
    }
}

pub fn lstat(p: &Path) -> io::Result<FileAttr> {
    stat(p) // симлинков нет
}

pub fn canonicalize(p: &Path) -> io::Result<PathBuf> {
    match root_name(p)? {
        None => Ok(PathBuf::from("/")),
        Some(name) => {
            stat(p)?; // canonicalize требует существования
            let mut s = String::with_capacity(1 + name.len());
            s.push('/');
            s.push_str(name);
            Ok(PathBuf::from(s))
        }
    }
}
