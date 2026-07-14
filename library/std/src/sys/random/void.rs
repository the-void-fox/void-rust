//! «Случайность» VOID (Веха 31): xorshift от монотонного счётчика — НЕ
//! криптографическая, годится для сидов HashMap. Честный источник энтропии —
//! отдельная задача (у QEMU-машины VOID нет virtio-rng… пока).

use crate::sys::pal::abi;

pub fn fill_bytes(bytes: &mut [u8]) {
    let mut x = abi::now_ns() | 1;
    for chunk in bytes.chunks_mut(8) {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let v = x.to_ne_bytes();
        chunk.copy_from_slice(&v[..chunk.len()]);
    }
}
