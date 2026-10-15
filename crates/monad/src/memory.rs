//! [MIP-3] linear memory costs with a pooled per-transaction memory limit.
//!
//! Port of `monad-revm` `memory/mod.rs`. Replaces the Ethereum quadratic memory cost formula
//! (`3·n + n²/512`) with a linear formula (`n / 2`, where `n` is the word count). MonadNine and
//! later enforce the memory limit (8 MiB by default) across the whole call stack: a frame's memory
//! counts against the limit together with the memory held by every suspended ancestor frame, as in
//! revm's shared memory buffer. The ancestor total is tracked in
//! [`MonadEvmExt::memory_pool_offset`](crate::MonadEvmExt).
//!
//! [MIP-3]: https://mips.monad.xyz/MIPs/MIP-3

use crate::MonadEvmTypes;
use evm2::{
    interpreter::{Gas, InstrStop, InterpreterState, Result},
    utils::num_words,
};

/// MIP-3 memory expansion cost: `num_words / 2`.
///
/// For 8 MiB (262 144 words) the total cost is 131 072 gas.
#[inline]
pub const fn monad_memory_cost(num_words: usize) -> u64 {
    (num_words as u64) >> 1
}

/// Returns whether growing the current frame to `new_words` exceeds the pooled memory limit.
#[inline]
pub const fn pooled_limit_reached(pool_offset: u64, new_words: usize, limit: u64) -> bool {
    pool_offset.saturating_add((new_words as u64).saturating_mul(32)) > limit
}

/// Resizes frame memory to cover `offset..offset + len` using the MIP-3 linear cost model.
///
/// The pooled memory limit is checked first, matching `monad_resize_memory` with the
/// `memory_limit` feature.
#[inline]
pub fn monad_resize_memory(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MonadEvmTypes>,
    offset: usize,
    len: usize,
) -> Result {
    let new_num_words = num_words(offset.saturating_add(len));
    let limit = state.version().memory_limit;
    let pool_offset = state.host().ext().memory_pool_offset;
    if pooled_limit_reached(pool_offset, new_num_words, limit) {
        return Err(InstrStop::MemoryLimitOOG);
    }

    if new_num_words > gas.memory().words_num {
        return monad_resize_memory_cold(gas, state, new_num_words);
    }

    Ok(())
}

#[cold]
#[inline(never)]
fn monad_resize_memory_cold(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MonadEvmTypes>,
    new_num_words: usize,
) -> Result {
    let Some(new_size) = new_num_words.checked_mul(32) else {
        return Err(InstrStop::MemoryOOG);
    };
    let total_cost = monad_memory_cost(new_num_words);
    // `new_num_words > words_num` guarantees `total_cost >= expansion_cost`.
    let delta = gas.memory_mut().set_words_num(new_num_words, total_cost).unwrap_or_default();
    gas.spend(delta).map_err(|_| InstrStop::MemoryOOG)?;
    state.memory().resize(0, new_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_cost_matches_monad_revm() {
        for words in [0usize, 1, 2, 3, 10, 100, 4097, 262_144, 262_145] {
            assert_eq!(monad_memory_cost(words), monad_revm::memory::monad_memory_cost(words));
        }
        assert_eq!(monad_memory_cost(262_144), 131_072);
    }

    #[test]
    fn pooled_limit_matches_shared_memory_rule() {
        // revm `SharedMemory::limit_reached`: `checkpoint + new_words * 32 > limit`.
        let limit = crate::MONAD_MEMORY_LIMIT;
        assert!(!pooled_limit_reached(0, 262_144, limit));
        assert!(pooled_limit_reached(0, 262_145, limit));
        assert!(pooled_limit_reached(32, 262_144, limit));
        assert!(!pooled_limit_reached(limit - 64, 2, limit));
        assert!(pooled_limit_reached(limit - 64, 3, limit));
    }
}
