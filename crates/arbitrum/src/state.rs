//! ArbOS state storage.
//!
//! Port of the storage layout of Nitro's `arbos/storage`, `arbos/arbosState`, `arbos/l1pricing`,
//! `arbos/l2pricing`, `arbos/blockhash` and `arbos/features`. ArbOS keeps its state in the storage
//! of [`ARBOS_STATE_ADDRESS`], split into subspaces whose slots are hashed from the subspace key,
//! so it can be read from (and written to) any database holding the chain state, including a fork.

use crate::ARBOS_STATE_ADDRESS;
use alloy_primitives::{Address, B256, U256, keccak256};
use evm2::{DatabaseError, evm::State};

/// Root ArbOS state offsets (`arbosState` `Offset`s).
pub mod root {
    /// ArbOS version.
    pub const VERSION: u64 = 0;
    /// Network fee account.
    pub const NETWORK_FEE_ACCOUNT: u64 = 3;
    /// Chain ID.
    pub const CHAIN_ID: u64 = 4;
    /// Infrastructure fee account.
    pub const INFRA_FEE_ACCOUNT: u64 = 6;
    /// Brotli compression level of the poster fee estimate.
    pub const BROTLI_COMPRESSION_LEVEL: u64 = 7;
    /// Whether tips are collected, from ArbOS 60.
    pub const COLLECT_TIPS: u64 = 11;
}

/// Root ArbOS state subspace IDs.
pub mod subspace {
    /// L1 pricing state.
    pub const L1_PRICING: u8 = 0;
    /// L2 pricing state.
    pub const L2_PRICING: u8 = 1;
    /// L1 block hashes.
    pub const BLOCKHASHES: u8 = 6;
    /// Feature flags.
    pub const FEATURES: u8 = 9;
}

/// L1 pricing state offsets (`l1pricing`).
pub mod l1_pricing {
    /// L1 pricer reward recipient.
    pub const PAY_REWARDS_TO: u64 = 0;
    /// Equilibration units.
    pub const EQUILIBRATION_UNITS: u64 = 1;
    /// Price inertia.
    pub const INERTIA: u64 = 2;
    /// Reward per unit.
    pub const PER_UNIT_REWARD: u64 = 3;
    /// Last update time.
    pub const LAST_UPDATE_TIME: u64 = 4;
    /// Funds due for rewards (signed).
    pub const FUNDS_DUE_FOR_REWARDS: u64 = 5;
    /// Calldata units since the last update.
    pub const UNITS_SINCE_UPDATE: u64 = 6;
    /// Price per calldata unit, the L1 base fee estimate.
    pub const PRICE_PER_UNIT: u64 = 7;
    /// Last surplus (signed).
    pub const LAST_SURPLUS: u64 = 8;
    /// Per-batch gas cost (signed 64-bit).
    pub const PER_BATCH_GAS_COST: u64 = 9;
    /// Amortized cost cap in basis points.
    pub const AMORTIZED_COST_CAP_BIPS: u64 = 10;
    /// L1 fees available.
    pub const L1_FEES_AVAILABLE: u64 = 11;
}

/// L2 pricing state offsets (`l2pricing`).
pub mod l2_pricing {
    /// Speed limit per second.
    pub const SPEED_LIMIT_PER_SECOND: u64 = 0;
    /// Per-block gas limit (the transaction gas cap before ArbOS 50).
    pub const PER_BLOCK_GAS_LIMIT: u64 = 1;
    /// L2 base fee.
    pub const BASE_FEE_WEI: u64 = 2;
    /// Minimum L2 base fee.
    pub const MIN_BASE_FEE_WEI: u64 = 3;
    /// Legacy gas backlog.
    pub const GAS_BACKLOG: u64 = 4;
    /// Pricing inertia.
    pub const PRICING_INERTIA: u64 = 5;
    /// Backlog tolerance.
    pub const BACKLOG_TOLERANCE: u64 = 6;
    /// Per-transaction gas limit, from ArbOS 50.
    pub const PER_TX_GAS_LIMIT: u64 = 7;
    /// Sub-storage ID of the single-gas pricing constraints.
    pub const GAS_CONSTRAINTS: u8 = 0;
    /// Sub-storage ID of the multi-gas pricing constraints.
    pub const MULTI_GAS_CONSTRAINTS: u8 = 1;
    /// Backlog offset inside a single-gas constraint.
    pub const CONSTRAINT_BACKLOG: u64 = 2;
}

/// A subspace of the ArbOS state storage (Nitro's `storage.Storage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArbOsStorage {
    /// Subspace key. The root subspace has an empty key.
    key: Option<B256>,
}

impl ArbOsStorage {
    /// Root ArbOS state storage.
    pub const ROOT: Self = Self { key: None };

    fn key(&self) -> &[u8] {
        self.key.as_ref().map_or(&[], |key| key.as_slice())
    }

    /// Opens the sub-storage with `id`.
    pub fn open(&self, id: &[u8]) -> Self {
        Self { key: Some(keccak256([self.key(), id].concat())) }
    }

    /// Returns the storage slot of `offset`.
    ///
    /// Nitro hashes the subspace key with the first 31 bytes of the offset and keeps its last byte,
    /// so 256 consecutive offsets share a hash.
    pub fn slot(&self, offset: U256) -> U256 {
        let offset = offset.to_be_bytes::<32>();
        let hash = keccak256([self.key(), &offset[..31]].concat());
        let mut slot = [0; 32];
        slot[..31].copy_from_slice(&hash[..31]);
        slot[31] = offset[31];
        U256::from_be_bytes(slot)
    }

    /// Returns the storage slot of a `u64` offset.
    pub fn slot_u64(&self, offset: u64) -> U256 {
        self.slot(U256::from(offset))
    }

    /// Reads the value at `offset` without loading it into the transaction overlay.
    pub fn get(&self, state: &mut State<'_>, offset: u64) -> Result<U256, DatabaseError> {
        state.storage_slot_untracked(&ARBOS_STATE_ADDRESS, &self.slot_u64(offset))
    }

    /// Reads a `uint64` (Nitro reads the low 64 bits).
    pub fn get_u64(&self, state: &mut State<'_>, offset: u64) -> Result<u64, DatabaseError> {
        self.get(state, offset).map(|value| value.as_limbs()[0])
    }

    /// Reads a signed 64-bit integer.
    pub fn get_i64(&self, state: &mut State<'_>, offset: u64) -> Result<i64, DatabaseError> {
        self.get_u64(state, offset).map(|value| value as i64)
    }

    /// Reads an address (the low 20 bytes).
    pub fn get_address(
        &self,
        state: &mut State<'_>,
        offset: u64,
    ) -> Result<Address, DatabaseError> {
        self.get(state, offset).map(|value| Address::from_word(value.into()))
    }

    /// Writes `value` at `offset` as a journaled change.
    pub fn set(
        &self,
        state: &mut State<'_>,
        offset: u64,
        value: U256,
    ) -> Result<(), DatabaseError> {
        state.storage_slot(&ARBOS_STATE_ADDRESS, self.slot_u64(offset))?.set(value);
        Ok(())
    }
}

/// Returns the L1 pricing storage.
pub fn l1_pricing_storage() -> ArbOsStorage {
    ArbOsStorage::ROOT.open(&[subspace::L1_PRICING])
}

/// Returns the L2 pricing storage.
pub fn l2_pricing_storage() -> ArbOsStorage {
    ArbOsStorage::ROOT.open(&[subspace::L2_PRICING])
}

/// Returns the L1 block hash storage. Offset 0 holds the current L1 block number, and offset
/// `1 + n % 256` the hash of L1 block `n`.
pub fn blockhashes_storage() -> ArbOsStorage {
    ArbOsStorage::ROOT.open(&[subspace::BLOCKHASHES])
}

/// Returns the feature flag storage.
pub fn features_storage() -> ArbOsStorage {
    ArbOsStorage::ROOT.open(&[subspace::FEATURES])
}

/// Returns the storage of element `index` of a sub-storage vector (Nitro's `SubStorageVector`).
///
/// Offset 0 of the vector storage holds its length.
pub fn vector_element(vector: &ArbOsStorage, index: u64) -> ArbOsStorage {
    vector.open(&index.to_be_bytes())
}

/// Returns the L1 block number recorded in the ArbOS state, which the `NUMBER` opcode returns.
pub fn l1_block_number(state: &mut State<'_>) -> Result<u64, DatabaseError> {
    blockhashes_storage().get_u64(state, 0)
}

/// Returns the hash of L1 block `number` recorded in the ArbOS state, or `None` when it is not
/// among the 256 blocks before `current`, the current L1 block number.
pub fn l1_block_hash(
    state: &mut State<'_>,
    current: u64,
    number: u64,
) -> Result<Option<B256>, DatabaseError> {
    if number >= current || number.saturating_add(256) < current {
        return Ok(None);
    }
    blockhashes_storage().get(state, 1 + number % 256).map(|hash| Some(hash.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::uint;

    #[test]
    fn slot_mapping_matches_nitro() {
        // Slots at which `eth_getStorageAt` on Arbitrum One returns the expected values (ArbOS
        // version 61, chain ID 42161, a 20 gwei minimum base fee and six gas constraints).
        let gas_constraints = l2_pricing_storage().open(&[l2_pricing::GAS_CONSTRAINTS]);
        for (slot, expected) in [
            (
                ArbOsStorage::ROOT.slot_u64(root::VERSION),
                uint!(0x15fed0451499512d95f3ec5a41c878b9de55f21878b5b4e190d4667ec709b400_U256),
            ),
            (
                ArbOsStorage::ROOT.slot_u64(root::CHAIN_ID),
                uint!(0x15fed0451499512d95f3ec5a41c878b9de55f21878b5b4e190d4667ec709b404_U256),
            ),
            (
                l2_pricing_storage().slot_u64(l2_pricing::MIN_BASE_FEE_WEI),
                uint!(0xe54de2a4cdacc0a0059d2b6e16348103df8c4aff409c31e40ec73d11926c8203_U256),
            ),
            (
                gas_constraints.slot_u64(0),
                uint!(0x52232757544c763eb1c1511dc67b97f352c88a089b72a204b89e1c66fc278300_U256),
            ),
            (
                vector_element(&gas_constraints, 2).slot_u64(l2_pricing::CONSTRAINT_BACKLOG),
                uint!(0x15feb681c0d80d6dd4522f2158aecf87aabe907423539f227812e8c90210b402_U256),
            ),
            (
                blockhashes_storage().slot_u64(6),
                uint!(0x3c79da47f96b0f39664f73c0a1f350580be90742947dddfa21ba64d578dfe606_U256),
            ),
        ] {
            assert_eq!(slot, expected);
        }
    }
}
