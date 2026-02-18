//! Bit-packed I/O helpers for digital I/O banks.
//!
//! Digital inputs and outputs are stored as `[u64; 16]` banks (1024 bits),
//! one bit per pin. These helpers provide ergonomic access to individual bits.
//!
//! ## Layout
//!
//! `bank[i]` holds pins `(i*64)..((i+1)*64)`.
//! Bit 0 of `bank[0]` = pin 0, bit 63 of `bank[0]` = pin 63,
//! bit 0 of `bank[1]` = pin 64, etc.
//!
//! ## Performance
//!
//! All operations are branchless and inline — suitable for RT loop usage.

use crate::consts::{MAX_DI, MAX_DO};

/// Number of `u64` words in a DI/DO bank: `1024 / 64 = 16`.
pub const BANK_WORDS: usize = 16;

/// Read a single digital input bit from a packed bank.
///
/// # Arguments
/// - `bank`: The packed bit bank (`[u64; BANK_WORDS]`).
/// - `pin`: Pin index (0..1023).
///
/// # Returns
/// `true` if the bit is set, `false` otherwise.
///
/// # Panics
/// Panics in debug mode if `pin >= MAX_DI`.
#[inline]
pub fn get_di(bank: &[u64; BANK_WORDS], pin: usize) -> bool {
    debug_assert!(pin < MAX_DI, "DI pin index {pin} out of range (max {MAX_DI})");
    let word = pin / 64;
    let bit = pin % 64;
    (bank[word] >> bit) & 1 == 1
}

/// Set a single digital output bit in a packed bank.
///
/// # Arguments
/// - `bank`: The packed bit bank (`[u64; BANK_WORDS]`).
/// - `pin`: Pin index (0..1023).
/// - `value`: `true` to set the bit, `false` to clear it.
///
/// # Panics
/// Panics in debug mode if `pin >= MAX_DO`.
#[inline]
pub fn set_do(bank: &mut [u64; BANK_WORDS], pin: usize, value: bool) {
    debug_assert!(pin < MAX_DO, "DO pin index {pin} out of range (max {MAX_DO})");
    let word = pin / 64;
    let bit = pin % 64;
    if value {
        bank[word] |= 1u64 << bit;
    } else {
        bank[word] &= !(1u64 << bit);
    }
}

/// Pack a slice of booleans into a `u64` bank.
///
/// Up to `BANK_WORDS * 64 = 1024` booleans. Extra booleans are ignored.
/// Missing booleans leave bits at 0.
///
/// # Arguments
/// - `bools`: Slice of boolean values to pack.
/// - `bank`: Output packed bank.
#[inline]
pub fn pack_bools(bools: &[bool], bank: &mut [u64; BANK_WORDS]) {
    *bank = [0u64; BANK_WORDS];
    let count = bools.len().min(BANK_WORDS * 64);
    for (i, &val) in bools[..count].iter().enumerate() {
        if val {
            let word = i / 64;
            let bit = i % 64;
            bank[word] |= 1u64 << bit;
        }
    }
}

/// Unpack a `u64` bank into a boolean slice.
///
/// Writes up to `out.len()` booleans.
///
/// # Arguments
/// - `bank`: The packed bit bank.
/// - `out`: Output boolean slice.
#[inline]
pub fn unpack_bools(bank: &[u64; BANK_WORDS], out: &mut [bool]) {
    let count = out.len().min(BANK_WORDS * 64);
    for (i, val) in out[..count].iter_mut().enumerate() {
        let word = i / 64;
        let bit = i % 64;
        *val = (bank[word] >> bit) & 1 == 1;
    }
}

/// Count the number of set bits in a bank (population count).
///
/// Useful for diagnostics (e.g., "how many DIs are active?").
#[inline]
pub fn count_set(bank: &[u64; BANK_WORDS]) -> u32 {
    let mut total = 0u32;
    for &word in bank {
        total += word.count_ones();
    }
    total
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_roundtrip() {
        let mut bank = [0u64; BANK_WORDS];

        // Set pin 0.
        set_do(&mut bank, 0, true);
        assert!(get_di(&bank, 0));
        assert!(!get_di(&bank, 1));

        // Set pin 63 (last bit of first word).
        set_do(&mut bank, 63, true);
        assert!(get_di(&bank, 63));

        // Set pin 64 (first bit of second word).
        set_do(&mut bank, 64, true);
        assert!(get_di(&bank, 64));
        assert!(!get_di(&bank, 65));

        // Set pin 1023 (last valid pin).
        set_do(&mut bank, 1023, true);
        assert!(get_di(&bank, 1023));

        // Clear pin 0.
        set_do(&mut bank, 0, false);
        assert!(!get_di(&bank, 0));
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let mut bools = vec![false; 1024];
        bools[0] = true;
        bools[7] = true;
        bools[64] = true;
        bools[1023] = true;

        let mut bank = [0u64; BANK_WORDS];
        pack_bools(&bools, &mut bank);

        assert!(get_di(&bank, 0));
        assert!(get_di(&bank, 7));
        assert!(!get_di(&bank, 8));
        assert!(get_di(&bank, 64));
        assert!(get_di(&bank, 1023));

        let mut out = vec![false; 1024];
        unpack_bools(&bank, &mut out);
        assert_eq!(bools, out);
    }

    #[test]
    fn count_set_works() {
        let mut bank = [0u64; BANK_WORDS];
        assert_eq!(count_set(&bank), 0);

        set_do(&mut bank, 0, true);
        set_do(&mut bank, 100, true);
        set_do(&mut bank, 999, true);
        assert_eq!(count_set(&bank), 3);

        bank = [u64::MAX; BANK_WORDS];
        assert_eq!(count_set(&bank), 1024);
    }

    #[test]
    fn empty_pack() {
        let mut bank = [u64::MAX; BANK_WORDS];
        pack_bools(&[], &mut bank);
        assert_eq!(count_set(&bank), 0);
    }

    #[test]
    fn partial_pack() {
        let bools = vec![true; 10];
        let mut bank = [0u64; BANK_WORDS];
        pack_bools(&bools, &mut bank);
        assert_eq!(count_set(&bank), 10);
        // Bits 0..9 set, 10..1023 clear.
        for i in 0..10 {
            assert!(get_di(&bank, i));
        }
        assert!(!get_di(&bank, 10));
    }
}
