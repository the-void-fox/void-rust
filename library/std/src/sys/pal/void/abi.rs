//! Syscall-ABI VOID (v2) — единственное место, где std видит инструкцию syscall'а.
//!
//! Зеркало `Code/programs/user/src/lib.rs` репозитория VOID: номера и раскладка
//! регистров обязаны совпадать с `kernel/src/proc.rs` и `kernel/src/arch/*/trap.rs`.
//! riscv64: номер в a7, аргументы a0..a6, результаты a0..a3 (`ecall`).
//! x86_64: номер в rax, аргументы rdi,rsi,rdx,r10,r8,r9, результаты rax,rdi,rsi,rdx
//! (`int 0x80`; седьмой аргумент rbx здесь не нужен — его использует только SYS_CALL).

pub const SYS_WRITE: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_YIELD: usize = 3;
pub const SYS_CALL: usize = 5;
pub const SYS_READ: usize = 14;
pub const SYS_MAP: usize = 17;
pub const SYS_ARGS: usize = 18;
pub const SYS_STARTCAP: usize = 19;
// Веха 35 — нити.
pub const SYS_THREAD_SPAWN: usize = 23;
pub const SYS_THREAD_EXIT: usize = 24;
pub const SYS_THREAD_JOIN: usize = 25;
pub const SYS_FUTEX: usize = 26;
pub const SYS_SET_TLS: usize = 27;

/// «Права нет»: и как аргумент SYS_CALL (не передавать capability), и как
/// ответ SYS_STARTCAP (слот пуст).
pub const NO_CAP: usize = usize::MAX;

/// Наносекунд в тике [`now_ns`]: riscv — таймбаза QEMU virt 10 МГц; x86 — TSC
/// (~1 ГГц в QEMU TCG; на железе потребует калибровки).
#[cfg(target_arch = "riscv64")]
const TICK_NS: u64 = 100;
#[cfg(target_arch = "x86_64")]
const TICK_NS: u64 = 1;

#[cfg(target_arch = "riscv64")]
#[inline(always)]
fn syscall(
    num: usize,
    mut a0: usize,
    mut a1: usize,
    a2: usize,
    a3: usize,
) -> (usize, usize) {
    unsafe {
        core::arch::asm!("ecall", in("a7") num,
            inout("a0") a0, inout("a1") a1, in("a2") a2, in("a3") a3,
            options(nostack));
    }
    (a0, a1)
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn syscall(
    num: usize,
    mut a0: usize,
    mut a1: usize,
    a2: usize,
    a3: usize,
) -> (usize, usize) {
    let r0;
    unsafe {
        core::arch::asm!("int 0x80",
            inout("rax") num => r0,
            inout("rdi") a0, inout("rsi") a1, in("rdx") a2, in("r10") a3,
            options(nostack));
    }
    let _ = a1;
    (r0, a0)
}

/// Полный 7-аргументный syscall — нужен только SYS_CALL (IPC). riscv: a0..a6.
/// x86: rdi,rsi,rdx,r10,r8,r9,rbx — rbx нельзя назвать операндом `asm!`
/// (резерв LLVM), 7-й аргумент заезжает в него через `xchg` и выезжает обратно.
#[cfg(target_arch = "riscv64")]
#[inline(always)]
fn syscall7(
    num: usize,
    mut a0: usize,
    mut a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> (usize, usize) {
    unsafe {
        core::arch::asm!("ecall", in("a7") num,
            inout("a0") a0, inout("a1") a1, in("a2") a2, in("a3") a3,
            in("a4") a4, in("a5") a5, in("a6") a6,
            options(nostack));
    }
    (a0, a1)
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn syscall7(
    num: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> (usize, usize) {
    let (r0, r1);
    unsafe {
        core::arch::asm!(
            "xchg rbx, {a6}",
            "int 0x80",
            "xchg rbx, {a6}",
            a6 = inout(reg) a6 => _,
            inout("rax") num => r0,
            inout("rdi") a0 => r1,
            inout("rsi") a1 => _,
            inout("rdx") a2 => _,
            in("r10") a3, in("r8") a4, in("r9") a5,
            options(nostack),
        );
    }
    (r0, r1)
}

pub fn sys_write(buf: &[u8]) -> usize {
    syscall(SYS_WRITE, buf.as_ptr() as usize, buf.len(), 0, 0).0
}

/// Блокируется до появления ввода; возвращает прочитанное (минимум 1 байт).
pub fn sys_read(buf: &mut [u8]) -> usize {
    syscall(SYS_READ, buf.as_mut_ptr() as usize, buf.len(), 0, 0).0
}

pub fn sys_exit(code: usize) -> ! {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("ecall", in("a7") SYS_EXIT, in("a0") code,
            options(nostack, noreturn))
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("int 0x80", in("rax") SYS_EXIT, in("rdi") code,
            options(nostack, noreturn))
    }
}

pub fn sys_yield() {
    syscall(SYS_YIELD, 0, 0, 0, 0);
}

/// Лениво зарезервировать `len` байт кучи; VA начала или usize::MAX.
/// Страницы придут по page fault — обнулёнными.
pub fn sys_map(len: usize) -> usize {
    syscall(SYS_MAP, len, 0, 0, 0).0
}

/// argv (`sel`=0) или env (`sel`=1) процесса: NUL-разделённые записи в `buf`
/// (обрезаются по его размеру), возвращается ПОЛНАЯ длина блоба.
pub fn sys_args(sel: usize, buf: &mut [u8]) -> usize {
    syscall(SYS_ARGS, sel, buf.as_mut_ptr() as usize, buf.len(), 0).0
}

/// `SYS_CALL` без передачи права: послать `send` эндпоинту `ep`, ждать ответа
/// в `recv`. Возвращает байты ответа (усечённые по `recv`) или `usize::MAX`.
pub fn sys_call(ep: usize, op: usize, send: &[u8], recv: &mut [u8]) -> usize {
    syscall7(
        SYS_CALL,
        ep,
        op,
        send.as_ptr() as usize,
        send.len(),
        recv.as_mut_ptr() as usize,
        recv.len(),
        NO_CAP,
    )
    .0
}

/// Стартовая capability из таблицы наследования (Веха 30, «preopen»-модель):
/// слот 0 у детей vsh — эндпоинт посикс-персоналии, слот 1 — право EXEC.
/// Пустой слот — [`NO_CAP`].
pub fn sys_start_cap(i: usize) -> usize {
    syscall(SYS_STARTCAP, i, 0, 0, 0).0
}

/// Наносекунд в тик (для перевода futex-таймаутов в единицы `arch::now_ticks` ядра).
pub const fn tick_ns() -> u64 {
    TICK_NS
}

// ─── нити (Веха 35) ───────────────────────────────────────────────────────────

/// `SYS_THREAD_SPAWN(entry, arg, stack_top)` -> id нити | MAX. Нить исполняет
/// `entry(arg)` на стеке `stack_top` в том же адресном пространстве и домене.
pub fn sys_thread_spawn(entry: usize, arg: usize, stack_top: usize) -> usize {
    syscall(SYS_THREAD_SPAWN, entry, arg, stack_top, 0).0
}

/// `SYS_THREAD_EXIT(retval)` — завершить текущую нить (не процесс).
pub fn sys_thread_exit(retval: usize) -> ! {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("ecall", in("a7") SYS_THREAD_EXIT, in("a0") retval,
            options(nostack, noreturn))
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("int 0x80", in("rax") SYS_THREAD_EXIT, in("rdi") retval,
            options(nostack, noreturn))
    }
}

/// `SYS_THREAD_JOIN(tid)` -> retval нити | MAX.
pub fn sys_thread_join(tid: usize) -> usize {
    syscall(SYS_THREAD_JOIN, tid, 0, 0, 0).0
}

/// `SYS_FUTEX` WAIT(uaddr, expected, timeout_ticks): уснуть, пока `*uaddr == expected`.
/// `timeout_ticks` == 0 — бессрочно. Возврат: 0 — разбужены, 1 — таймаут.
pub fn sys_futex_wait(uaddr: *const u32, expected: u32, timeout_ticks: usize) -> usize {
    syscall(SYS_FUTEX, 0, uaddr as usize, expected as usize, timeout_ticks).0
}

/// `SYS_FUTEX` WAKE(uaddr, count): разбудить до `count` спящих; вернуть число.
pub fn sys_futex_wake(uaddr: *const u32, count: usize) -> usize {
    syscall(SYS_FUTEX, 1, uaddr as usize, count, 0).0
}

/// `SYS_SET_TLS(ptr)` — задать TLS-указатель нити (tp / база %fs).
pub fn sys_set_tls(ptr: usize) {
    syscall(SYS_SET_TLS, ptr, 0, 0, 0);
}

/// Монотонное время из U-mode (не syscall): rdtime / rdtsc, пересчитанные в нс.
pub fn now_ns() -> u64 {
    #[cfg(target_arch = "riscv64")]
    let ticks = {
        let t: u64;
        unsafe { core::arch::asm!("rdtime {0}", out(reg) t, options(nomem, nostack)) };
        t
    };
    #[cfg(target_arch = "x86_64")]
    let ticks = {
        let (lo, hi): (u32, u32);
        unsafe {
            core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack))
        };
        (hi as u64) << 32 | lo as u64
    };
    ticks * TICK_NS
}
