//! [MIP-8] page-based storage access tracking and gas accounting.
//!
//! Port of `monad-revm` `page.rs` and the page-aware `sload`/`sstore` in `journal.rs`.
//!
//! `monad-revm` keeps a `PageAccessTracker` with its own change journal that is checkpointed and
//! reverted alongside every call frame. evm2 has no frame checkpoint hook for extension state, so
//! the tracker lives in transaction-scoped transient storage of a reserved address
//! ([`PAGE_TRACKER_ADDRESS`]). evm2 journals and rolls back transient storage with each frame and
//! clears it at the end of every transaction, which gives exactly the `monad-revm` lifetime. The
//! reserved address has no code, so no contract can observe or modify it.
//!
//! [MIP-8]: https://mips.monad.xyz/MIPs/MIP-8

use crate::MonadEvmTypes;
use alloy_primitives::{Address, U256, address, keccak256};
use evm2::{Evm, evm::SStore};

/// Number of bits shifted right to derive a 4 KiB page index from a storage slot.
pub const PAGE_SHIFT: usize = 7;

/// Number of 32-byte storage words in a page.
pub const WORDS_PER_PAGE: u64 = 1 << PAGE_SHIFT;

/// Base cost charged for every storage access.
pub const BASE_COST: u64 = 100;

/// Cost charged for the first write to a page in a transaction.
pub const PAGE_WRITE_COST: u64 = 2_800;

/// Cost charged when a page reaches a new high-water mark of net state growth.
pub const STATE_GROWTH_COST: u64 = 17_000;

/// Reserved address whose transient storage holds the MIP-8 page tracker.
pub const PAGE_TRACKER_ADDRESS: Address = address!("0x4d4950382d706167652d747261636b65722d3030");

/// Returns the page index for a storage slot.
#[inline]
pub fn page_index(slot: U256) -> U256 {
    slot >> PAGE_SHIFT
}

/// Returns the word offset of a storage slot within its page.
#[inline]
pub fn page_offset(slot: U256) -> U256 {
    slot & U256::from(WORDS_PER_PAGE - 1)
}

/// Transaction-local page key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StoragePageKey {
    /// Account address owning the storage page.
    pub address: Address,
    /// Page index derived from the storage slot.
    pub page: U256,
}

impl StoragePageKey {
    /// Creates a page key from an account address and storage slot.
    #[inline]
    pub fn from_slot(address: Address, slot: U256) -> Self {
        Self { address, page: page_index(slot) }
    }

    fn tracker_slot(&self, field: Field) -> U256 {
        let mut preimage = [0u8; 53];
        preimage[0] = field as u8;
        preimage[1..21].copy_from_slice(self.address.as_slice());
        preimage[21..].copy_from_slice(&self.page.to_be_bytes::<32>());
        keccak256(preimage).into()
    }
}

/// Page tracker fields stored in transient storage.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
enum Field {
    ReadAccessed = 1,
    WriteAccessed = 2,
    CurrentStateGrowth = 3,
    NetStateGrowth = 4,
}

fn load(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey, field: Field) -> U256 {
    evm.state_mut().tload(&PAGE_TRACKER_ADDRESS, &key.tracker_slot(field))
}

fn store(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey, field: Field, value: U256) {
    evm.state_mut().tstore(&PAGE_TRACKER_ADDRESS, &key.tracker_slot(field), &value);
}

fn load_counter(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey, field: Field) -> i32 {
    load(evm, key, field).as_limbs()[0] as u32 as i32
}

fn store_counter(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey, field: Field, value: i32) {
    store(evm, key, field, U256::from(value as u32));
}

/// Returns `true` if a page has been read during the current transaction.
pub fn is_read_accessed(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey) -> bool {
    !load(evm, key, Field::ReadAccessed).is_zero()
}

/// Marks a page as read-accessed. The change is reverted with the current frame.
pub fn mark_read_accessed(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey) {
    store(evm, key, Field::ReadAccessed, U256::ONE);
}

/// Marks a page as write-accessed, returning `true` if it was not write-accessed yet.
fn mark_write_accessed(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey) -> bool {
    if !load(evm, key, Field::WriteAccessed).is_zero() {
        return false;
    }
    store(evm, key, Field::WriteAccessed, U256::ONE);
    true
}

/// Returns the MIP-8 dynamic `SSTORE` cost for a storage transition and records page state.
pub fn sstore_gas(evm: &mut Evm<'_, MonadEvmTypes>, key: StoragePageKey, result: &SStore) -> u64 {
    let mut gas = BASE_COST;

    if result.new_value != result.present_value && mark_write_accessed(evm, key) {
        gas += PAGE_WRITE_COST;
    }

    let growth_delta = match (result.present_value.is_zero(), result.new_value.is_zero()) {
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    };
    if growth_delta == 0 {
        return gas;
    }

    let current_growth = load_counter(evm, key, Field::CurrentStateGrowth) + growth_delta;
    store_counter(evm, key, Field::CurrentStateGrowth, current_growth);

    if current_growth > load_counter(evm, key, Field::NetStateGrowth) {
        gas += STATE_GROWTH_COST;
        store_counter(evm, key, Field::NetStateGrowth, current_growth);
    }

    gas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_monad_revm() {
        use monad_revm::page;
        assert_eq!(PAGE_SHIFT, page::PAGE_SHIFT);
        assert_eq!(WORDS_PER_PAGE, page::WORDS_PER_PAGE);
        assert_eq!(BASE_COST, page::BASE_COST);
        assert_eq!(PAGE_WRITE_COST, page::PAGE_WRITE_COST);
        assert_eq!(STATE_GROWTH_COST, page::STATE_GROWTH_COST);
        for slot in [0u64, 1, 127, 128, 0x181, u64::MAX] {
            let slot = U256::from(slot);
            assert_eq!(page_index(slot), page::page_index(slot));
            assert_eq!(page_offset(slot), page::page_offset(slot));
        }
    }
}
