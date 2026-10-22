//! Arbitrum chains, system addresses and header metadata.
//!
//! Addresses are taken from Nitro's `core/types/arbitrum_signer.go` and `arbos/l1pricing`, and the
//! header layout from `core/types/arb_types.go`.

use crate::ArbOsVersion;
use alloy_primitives::{Address, B256, address};

/// Arbitrum One chain ID.
pub const ARBITRUM_ONE_CHAIN_ID: u64 = 42_161;

/// Arbitrum Nova chain ID.
pub const ARBITRUM_NOVA_CHAIN_ID: u64 = 42_170;

/// Arbitrum Sepolia chain ID.
pub const ARBITRUM_SEPOLIA_CHAIN_ID: u64 = 421_614;

/// Robinhood Chain mainnet chain ID (an Arbitrum Orbit chain).
pub const ROBINHOOD_MAINNET_CHAIN_ID: u64 = 4_663;

/// Robinhood Chain testnet chain ID (an Arbitrum Orbit chain).
pub const ROBINHOOD_TESTNET_CHAIN_ID: u64 = 46_630;

/// Returns whether `chain_id` is a known Arbitrum Nitro chain.
///
/// Covers Arbitrum One, Nova and Sepolia, and the Robinhood Chain Orbit chains. Other Orbit chains
/// run the same rules but are not recognized by ID.
pub const fn is_arbitrum_chain(chain_id: u64) -> bool {
    matches!(
        chain_id,
        ARBITRUM_ONE_CHAIN_ID
            | ARBITRUM_NOVA_CHAIN_ID
            | ARBITRUM_SEPOLIA_CHAIN_ID
            | ROBINHOOD_MAINNET_CHAIN_ID
            | ROBINHOOD_TESTNET_CHAIN_ID
    )
}

/// Account holding the ArbOS state in its storage.
pub const ARBOS_STATE_ADDRESS: Address = address!("0xA4B05FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF");

/// ArbOS system address, the sender of internal transactions.
pub const ARBOS_ADDRESS: Address = address!("0x00000000000000000000000000000000000a4b05");

/// Batch poster address, the block coinbase of sequenced (non-delayed-inbox) messages.
///
/// Only transactions in blocks with this coinbase pay the L1 poster fee, and tips are never
/// collected in blocks with another coinbase.
pub const BATCH_POSTER_ADDRESS: Address = address!("0xA4B000000000000000000073657175656e636572");

/// Pool receiving the L1 poster fee from ArbOS 2.
pub const L1_PRICER_FUNDS_POOL_ADDRESS: Address =
    address!("0xA4B00000000000000000000000000000000000f6");

/// Arbitrum Nitro block header metadata, packed into the header's `mixHash`.
///
/// Port of Nitro's `types.HeaderInfo`. The send root, stored in the header's `extraData`, is not
/// needed for execution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ArbHeaderInfo {
    /// Number of L2-to-L1 messages sent so far.
    pub send_count: u64,
    /// L1 block number returned by the `NUMBER` opcode.
    pub l1_block_number: u64,
    /// ArbOS version the block was executed with.
    pub arbos_version: ArbOsVersion,
    /// Whether the block collects tips. Execution reads the setting from the ArbOS state; this is
    /// the copy recorded in the header.
    pub collect_tips: bool,
}

impl ArbHeaderInfo {
    /// Decodes the metadata of a Nitro header from its `mixHash`.
    ///
    /// The caller must only use this for Nitro blocks: Nitro headers have a difficulty of `1` and
    /// a 32-byte `extraData`.
    pub fn from_mix_hash(mix_hash: B256) -> Self {
        let word = |range: core::ops::Range<usize>| {
            u64::from_be_bytes(mix_hash[range].try_into().expect("8 bytes"))
        };
        let arbos_version = ArbOsVersion::new(word(16..24));
        Self {
            send_count: word(0..8),
            l1_block_number: word(8..16),
            arbos_version,
            collect_tips: arbos_version == ArbOsVersion::V9 || mix_hash[25] & 1 == 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::b256;

    #[test]
    fn decodes_header_metadata() {
        // Arbitrum One block 517,283,379.
        let info = ArbHeaderInfo::from_mix_hash(b256!(
            "0x0000000000028d05000000000190890b000000000000003d0001000000000000"
        ));
        assert_eq!(
            info,
            ArbHeaderInfo {
                send_count: 0x28d05,
                l1_block_number: 0x190890b,
                arbos_version: ArbOsVersion::new(61),
                collect_tips: true,
            }
        );

        // ArbOS 9 always collected tips.
        let info = ArbHeaderInfo::from_mix_hash(b256!(
            "0x0000000000000001000000000000000200000000000000090000000000000000"
        ));
        assert_eq!(info.arbos_version, ArbOsVersion::V9);
        assert!(info.collect_tips);
    }

    #[test]
    fn recognizes_arbitrum_chains() {
        for chain_id in [42_161, 42_170, 421_614, 4_663, 46_630] {
            assert!(is_arbitrum_chain(chain_id), "{chain_id}");
        }
        assert!(!is_arbitrum_chain(1));
        assert!(!is_arbitrum_chain(10));
    }
}
