//! Native-TLS VOID (Веха 35): построить per-thread блок из шаблона `.tdata`/`.tbss` и
//! установить указатель нити. Модель — **local-exec** (статические no-PIE ELF): линкер
//! резолвит TPREL-смещения переменных ОТНОСИТЕЛЬНО начала TLS-сегмента, динамических
//! релокаций нет. Значит рантайму достаточно разложить копию сегмента и поставить
//! указатель так, как ждёт psABI архитектуры:
//!
//! - **riscv64 (Variant I):** `tp` (x4) — на НАЧАЛО блока; переменная по `tp + offset`.
//! - **x86-64 (Variant II):** база `%fs` — на TCB СРАЗУ ЗА блоком; переменная по
//!   `%fs + (offset − block_size)` (отрицательные смещения). `%fs:0` держит адрес TCB.
//!
//! Символы границ шаблона даёт линкер-скрипт программы (`programs/user/linker.ld`):
//! `__tls_start`/`__tdata_end`/`__tls_end` (адреса) и `__tls_align` (значение = адрес).
//! Блок нити не освобождается (деструкторов TLS у нас нет — `guard::enable` no-op).

use super::abi;
use crate::alloc::{GlobalAlloc, Layout, System};

unsafe extern "C" {
    static __tls_start: u8;
    static __tdata_end: u8;
    static __tls_end: u8;
    static __tls_align: u8;
}

#[inline]
fn round_up(x: usize, a: usize) -> usize {
    (x + a - 1) & !(a - 1)
}

/// Построить TLS-блок текущей нити и установить её указатель. Зовётся ПЕРВЫМ делом на
/// каждой нити (в `_start` главной и в трамплине дочерней) — до любого `#[thread_local]`.
///
/// # Safety
/// Вызывать ровно один раз на нить, до обращения к thread-local данным.
pub unsafe fn install() {
    // Провенанс: символ .tdata материализуем как настоящий указатель (это источник копии),
    // прочие адреса нужны лишь для арифметики — берём `.addr()` (strict provenance std).
    let start_ptr: *const u8 = core::ptr::addr_of!(__tls_start);
    let start = start_ptr.addr();
    let tdata_end = core::ptr::addr_of!(__tdata_end).addr();
    let tls_end = core::ptr::addr_of!(__tls_end).addr();
    // Значение символа `__tls_align` — его «адрес» (линкер задал = p_align сегмента PT_TLS,
    // MAX выравниваний секций). ЭТИМ выравниванием линкер округлял block_size при расчёте
    // TPREL-смещений (x86 Variant II), поэтому block_size считаем РОВНО по нему.
    let align = (core::ptr::addr_of!(__tls_align).addr()).max(1);

    let filesz = tdata_end - start; // инициализированная часть (.tdata)
    let memsz = tls_end - start; // весь шаблон (.tdata + .tbss)
    let block_size = round_up(memsz, align);

    // [ TLS-данные block_size | TCB (2 слова) ]. TCB нужен x86 (self-ptr в %fs:0); riscv
    // его не читает (dtv у нас нет), но выделяем единообразно. Само выделение выравниваем
    // ≥16 (минимум аллокатора) и ≥align — старт блока обязан быть кратен p_align.
    const TCB: usize = 2 * core::mem::size_of::<usize>();
    let alloc_align = align.max(16);
    let layout = Layout::from_size_align(block_size + TCB, alloc_align).unwrap();
    let mem = unsafe { System.alloc(layout) };
    if mem.is_null() {
        abi::sys_exit(99); // без TLS исполняться нельзя
    }

    // Скопировать шаблон, обнулить .tbss-хвост и TCB.
    unsafe {
        core::ptr::copy_nonoverlapping(start_ptr, mem, filesz);
        core::ptr::write_bytes(mem.add(filesz), 0, (block_size + TCB) - filesz);
    }

    #[cfg(target_arch = "riscv64")]
    let tp = mem.addr(); // Variant I: tp = начало блока

    #[cfg(target_arch = "x86_64")]
    let tp = {
        // Variant II: база %fs = TCB сразу за блоком; %fs:0 обязан хранить свой адрес
        // (пишем без предположения о выравнивании — align мог быть < размера слова).
        let tcb = unsafe { mem.add(block_size) } as *mut usize;
        let base = tcb.addr();
        unsafe { tcb.write_unaligned(base) };
        base
    };

    abi::sys_set_tls(tp);
}
