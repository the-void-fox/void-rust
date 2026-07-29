//! Случайность VOID — с Вехи 86 приходит ИЗ ЯДРА (`SYS_RANDOM`): аппаратный ГСЧ
//! (`RDRAND` на x86) плюс пул событий, перемешанные BLAKE3. Раньше здесь был
//! xorshift от монотонного счётчика, заведомо непригодный для криптографии.
//!
//! Оговорка остаётся для riscv: аппаратного источника у QEMU virt нет, поэтому
//! качество там ограничено накопленной энтропией событий (см. `kernel/src/random.rs`).

use crate::sys::pal::abi;

pub fn fill_bytes(bytes: &mut [u8]) {
    if bytes.is_empty() {
        return;
    }
    let n = abi::sys_random(bytes);
    if n == bytes.len() {
        return;
    }
    // Ядро не смогло заполнить буфер (старое ядро без SYS_RANDOM или недоступная память) —
    // не оставляем вызывающему нули: сиды HashMap на нулях предсказуемы. Запасной путь —
    // прежний xorshift от монотонного счётчика, с честной пометкой «не криптография».
    let mut x = abi::now_ns() | 1;
    for chunk in bytes.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let v = x.to_ne_bytes();
        chunk.copy_from_slice(&v[..chunk.len()]);
    }
}
