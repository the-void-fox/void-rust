//! Аллокатор VOID (Веха 31, MVP): bump над ленивой SYS_MAP-ареной.
//!
//! `dealloc` — no-op (утечка): для коротких программ (hello, утилиты uutils
//! в режиме «запустился-сделал-вышел») этого достаточно, а физические страницы
//! арены приходят от ядра ЛЕНИВО — нетронутый резерв ничего не стоит. Честный
//! free-list — когда появится долгоживущий потребитель. Потоков в VOID нет,
//! но атомики дешевле рассуждений о static mut.

use crate::alloc::{GlobalAlloc, Layout, System};
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::pal::abi;

/// Ленивый резерв: страницы выделяются ядром только по фактическому касанию.
const ARENA_SIZE: usize = 16 * 1024 * 1024;

static NEXT: AtomicUsize = AtomicUsize::new(0);
static END: AtomicUsize = AtomicUsize::new(0);

fn arena_next(size: usize, align: usize) -> *mut u8 {
    let mut next = NEXT.load(Ordering::Relaxed);
    if next == 0 {
        let base = abi::sys_map(ARENA_SIZE);
        if base == usize::MAX {
            return core::ptr::null_mut();
        }
        END.store(base + ARENA_SIZE, Ordering::Relaxed);
        next = base;
    }
    let aligned = (next + align - 1) & !(align - 1);
    let Some(new_next) = aligned.checked_add(size) else {
        return core::ptr::null_mut();
    };
    if new_next > END.load(Ordering::Relaxed) {
        return core::ptr::null_mut();
    }
    NEXT.store(new_next, Ordering::Relaxed);
    // Адрес пришёл из SYS_MAP (регистр из asm) — единственный источник
    // провенанса здесь; strict provenance запрещает голый `as`-каст.
    crate::ptr::with_exposed_provenance_mut(aligned)
}

#[stable(feature = "alloc_system_type", since = "1.28.0")]
unsafe impl GlobalAlloc for System {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        arena_next(layout.size(), layout.align())
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // Память арены не переиспользуется (dealloc — no-op), а свежие страницы
        // ядро выдаёт обнулёнными — alloc уже возвращает нули.
        arena_next(layout.size(), layout.align())
    }

    #[inline]
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
