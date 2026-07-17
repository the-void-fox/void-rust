//! Аллокатор VOID (Веха 32): first-fit free-list с коалесингом над ленивой
//! SYS_MAP-ареной.
//!
//! `GlobalAlloc::dealloc` получает `Layout`, поэтому занятые блоки НЕ несут
//! заголовков — метаданные («дырки» `[size | next]`) живут только в свободной
//! памяти. Список дырок упорядочен по адресу: освобождение сливает блок с
//! соседями, фрагментация не накапливается. Все адреса и размеры кратны
//! [`UNIT`] = 16 (минимальное выравнивание обеих архитектур, см. `MIN_ALIGN`),
//! так что любая дырка вмещает свой 16-байтный заголовок.
//!
//! Арена по-прежнему ленивая: страницы приходят от ядра по фактическому
//! касанию, нетронутый резерв ничего не стоит. Потоков в VOID нет — состояние
//! в `SyncUnsafeCell` без блокировки (как wasm без атомиков); alloc не
//! реентерабелен (сам памяти не просит).

use crate::alloc::{GlobalAlloc, Layout, System};
use crate::cell::SyncUnsafeCell;
use crate::ptr::with_exposed_provenance_mut;
use crate::sys::pal::abi;

/// Ленивый резерв: страницы выделяются ядром только по фактическому касанию.
const ARENA_SIZE: usize = 16 * 1024 * 1024;

/// Гранула аллокатора: минимальный блок, шаг размеров, гарантия выравнивания.
const UNIT: usize = 16;

struct Heap {
    /// Адрес первой дырки (0 — свободных нет).
    head: usize,
    /// Арена уже запрошена у ядра.
    init: bool,
}

static HEAP: SyncUnsafeCell<Heap> = SyncUnsafeCell::new(Heap { head: 0, init: false });

/// Заголовок дырки читается/пишется по голому адресу: память пришла из
/// SYS_MAP (регистр из asm) — вне модели памяти Rust, провенанс материализуем
/// через `with_exposed_provenance_mut` (как и в остальном pal).
#[inline]
unsafe fn hole(addr: usize) -> (usize, usize) {
    let p = with_exposed_provenance_mut::<usize>(addr);
    unsafe { (*p, *p.add(1)) }
}

#[inline]
unsafe fn hole_write(addr: usize, size: usize, next: usize) {
    let p = with_exposed_provenance_mut::<usize>(addr);
    unsafe {
        *p = size;
        *p.add(1) = next;
    }
}

/// Перевесить ссылку «после `prev`» на `to` (`prev` == 0 — это голова списка).
#[inline]
unsafe fn set_next(heap: &mut Heap, prev: usize, to: usize) {
    if prev == 0 {
        heap.head = to;
    } else {
        let (s, _) = unsafe { hole(prev) };
        unsafe { hole_write(prev, s, to) };
    }
}

/// Размер блока: минимум [`UNIT`], кратно [`UNIT`]. Единая формула в alloc и
/// dealloc — блок освобождается ровно того размера, какого выдавался.
#[inline]
fn block_size(layout: Layout) -> usize {
    layout.size().max(1).next_multiple_of(UNIT)
}

unsafe fn heap_alloc(layout: Layout) -> *mut u8 {
    let heap = unsafe { &mut *HEAP.get() };
    if !heap.init {
        let base = abi::sys_map(ARENA_SIZE);
        if base == usize::MAX {
            return crate::ptr::null_mut();
        }
        // Одна дырка на весь резерв: свежие страницы ядро выдаёт по касанию,
        // заголовок трогает только первую.
        unsafe { hole_write(base, ARENA_SIZE, 0) };
        heap.head = base;
        heap.init = true;
    }
    let size = block_size(layout);
    let align = layout.align().max(UNIT);

    let (mut prev, mut h) = (0usize, heap.head);
    while h != 0 {
        let (hs, hn) = unsafe { hole(h) };
        let aligned = (h + align - 1) & !(align - 1);
        let pad = aligned - h; // 0 или кратно UNIT — дырка-остаток валидна
        if pad + size <= hs {
            let tail = hs - pad - size;
            // Что останется в списке после выёма [aligned, aligned+size).
            let succ = if tail > 0 {
                unsafe { hole_write(aligned + size, tail, hn) };
                aligned + size
            } else {
                hn
            };
            if pad > 0 {
                unsafe { hole_write(h, pad, succ) };
            } else {
                unsafe { set_next(heap, prev, succ) };
            }
            return with_exposed_provenance_mut(aligned);
        }
        prev = h;
        h = hn;
    }
    crate::ptr::null_mut() // арена исчерпана — std честно увидит OOM
}

unsafe fn heap_dealloc(ptr: *mut u8, layout: Layout) {
    let heap = unsafe { &mut *HEAP.get() };
    let addr = ptr.addr();
    let size = block_size(layout);

    // Найти позицию в упорядоченном по адресу списке: prev < addr < h.
    let (mut prev, mut h) = (0usize, heap.head);
    while h != 0 && h < addr {
        let (_, hn) = unsafe { hole(h) };
        prev = h;
        h = hn;
    }
    // Слить с правым соседом, если блок кончается ровно там, где тот начинается.
    let (mut nsize, mut nnext) = (size, h);
    if h != 0 && addr + size == h {
        let (hs, hn) = unsafe { hole(h) };
        nsize = size + hs;
        nnext = hn;
    }
    // Слить с левым соседом — или встать в список самостоятельной дыркой.
    if prev != 0 {
        let (ps, _) = unsafe { hole(prev) };
        if prev + ps == addr {
            unsafe { hole_write(prev, ps + nsize, nnext) };
            return;
        }
    }
    unsafe {
        hole_write(addr, nsize, nnext);
        set_next(heap, prev, addr);
    }
}

#[stable(feature = "alloc_system_type", since = "1.28.0")]
unsafe impl GlobalAlloc for System {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { heap_alloc(layout) }
    }

    // alloc_zeroed/realloc — дефолтные (alloc + обнуление / alloc + копия +
    // dealloc): память теперь переиспользуется, «страницы приходят нулями»
    // больше не аргумент.

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { heap_dealloc(ptr, layout) }
    }
}
