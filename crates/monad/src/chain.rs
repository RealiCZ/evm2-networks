//! Monad chain context used for reserve-balance decisions.
//!
//! Port of `monad-revm` `chain.rs`.

use alloy_primitives::{Address, U256, map::HashSet};

/// Default max reserve balance: 10 MON.
pub const fn default_max_reserve_balance() -> U256 {
    U256::from_limbs([10_000_000_000_000_000_000u64, 0, 0, 0])
}

/// Monad chain context needed for reserve-balance decisions.
///
/// The default value contains no historical block metadata and is not a canonical replay context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonadChainContext {
    /// Combined senders and authorities from the parent block.
    pub parent_senders_and_authorities: HashSet<Address>,
    /// Combined senders and authorities from the grandparent block.
    pub grandparent_senders_and_authorities: HashSet<Address>,
    /// Senders already present in the current block.
    pub current_block_senders: Vec<Address>,
    /// Authorities already present in the current block.
    pub current_block_authorities: Vec<Vec<Address>>,
    /// Index of the current transaction within the current block metadata.
    pub current_tx_index: usize,
    /// Maximum reserve balance to enforce for accounts.
    pub max_reserve_balance: U256,
}

impl Default for MonadChainContext {
    fn default() -> Self {
        Self {
            parent_senders_and_authorities: HashSet::default(),
            grandparent_senders_and_authorities: HashSet::default(),
            current_block_senders: Vec::new(),
            current_block_authorities: Vec::new(),
            current_tx_index: 0,
            max_reserve_balance: default_max_reserve_balance(),
        }
    }
}

impl MonadChainContext {
    /// Returns the maximum reserve balance for the address.
    #[inline]
    pub const fn max_reserve_balance(&self, _address: Address) -> U256 {
        self.max_reserve_balance
    }

    /// Returns true if the sender may dip into reserve.
    pub fn sender_can_dip(&self, sender: Address, sender_is_delegated: bool) -> bool {
        if sender_is_delegated {
            return false;
        }

        if self.grandparent_senders_and_authorities.contains(&sender)
            || self.parent_senders_and_authorities.contains(&sender)
        {
            return false;
        }

        for (index, block_sender) in self.current_block_senders.iter().enumerate() {
            if index < self.current_tx_index && *block_sender == sender {
                return false;
            }
        }

        for (index, authorities) in self.current_block_authorities.iter().enumerate() {
            if index <= self.current_tx_index && authorities.contains(&sender) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_theirs(chain: &MonadChainContext) -> monad_revm::MonadChainContext {
        monad_revm::MonadChainContext {
            parent_senders_and_authorities: chain
                .parent_senders_and_authorities
                .iter()
                .copied()
                .collect(),
            grandparent_senders_and_authorities: chain
                .grandparent_senders_and_authorities
                .iter()
                .copied()
                .collect(),
            current_block_senders: chain.current_block_senders.clone(),
            current_block_authorities: chain.current_block_authorities.clone(),
            current_tx_index: chain.current_tx_index,
            max_reserve_balance: chain.max_reserve_balance,
        }
    }

    #[test]
    fn sender_can_dip_matches_monad_revm() {
        let a = Address::repeat_byte(1);
        let b = Address::repeat_byte(2);
        let c = Address::repeat_byte(3);
        let d = Address::repeat_byte(4);
        let chain = MonadChainContext {
            parent_senders_and_authorities: [a].into_iter().collect(),
            grandparent_senders_and_authorities: [b].into_iter().collect(),
            current_block_senders: vec![c, d, c],
            current_block_authorities: vec![vec![], vec![d], vec![a]],
            current_tx_index: 1,
            ..Default::default()
        };
        assert_eq!(default_max_reserve_balance(), monad_revm::chain::default_max_reserve_balance());
        let theirs = to_theirs(&chain);
        for index in 0..4 {
            let chain = MonadChainContext { current_tx_index: index, ..chain.clone() };
            let theirs =
                monad_revm::MonadChainContext { current_tx_index: index, ..theirs.clone() };
            for sender in [a, b, c, d, Address::repeat_byte(5)] {
                for delegated in [false, true] {
                    assert_eq!(
                        chain.sender_can_dip(sender, delegated),
                        theirs.sender_can_dip(sender, delegated),
                        "index {index} sender {sender} delegated {delegated}"
                    );
                }
            }
        }
    }
}
