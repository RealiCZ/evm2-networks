//! Monad hardfork identifiers.
//!
//! Port of `monad-revm` `spec.rs`.

use core::{fmt, str::FromStr};
use evm2::SpecId;

/// Monad mainnet chain ID.
pub const MONAD_MAINNET_CHAIN_ID: u64 = 143;

/// Monad testnet chain ID.
pub const MONAD_TESTNET_CHAIN_ID: u64 = 10143;

/// Monad hardfork identifier.
///
/// Variants are ordered by activation order. A lower discriminant is an earlier hardfork.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum MonadHardfork {
    /// Monad launch hardfork, based on Prague.
    MonadEight = 100,
    /// MIP-3, MIP-4 and MIP-5.
    MonadNine = 101,
    /// MIP-8 page-ified storage state.
    #[default]
    MonadTen = 102,
    /// Next development spec.
    MonadNext = 103,
}

impl MonadHardfork {
    /// All Monad hardforks in activation order.
    pub const ALL: [Self; 4] = [Self::MonadEight, Self::MonadNine, Self::MonadTen, Self::MonadNext];

    /// Returns the underlying Ethereum [`SpecId`] this Monad hardfork is built upon.
    pub const fn into_eth_spec(self) -> SpecId {
        match self {
            Self::MonadEight => SpecId::PRAGUE,
            Self::MonadNine | Self::MonadTen | Self::MonadNext => SpecId::OSAKA,
        }
    }

    /// Returns `true` if `self` is enabled when the active spec is `other`.
    pub const fn is_enabled_in(self, other: Self) -> bool {
        self as u8 <= other as u8
    }

    /// Returns the hardfork for a raw discriminant.
    pub const fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            100 => Some(Self::MonadEight),
            101 => Some(Self::MonadNine),
            102 => Some(Self::MonadTen),
            103 => Some(Self::MonadNext),
            _ => None,
        }
    }

    /// Returns the active Monad hardfork at the given timestamp for the specified chain.
    pub const fn from_chain_and_timestamp(chain_id: u64, timestamp: u64) -> Option<Self> {
        match chain_id {
            MONAD_MAINNET_CHAIN_ID => Some(Self::from_mainnet_timestamp(timestamp)),
            MONAD_TESTNET_CHAIN_ID => Some(Self::from_testnet_timestamp(timestamp)),
            _ => None,
        }
    }

    /// Retrieves the activation timestamp for this hardfork on mainnet.
    pub const fn mainnet_activation_timestamp(&self) -> Option<u64> {
        match self {
            Self::MonadEight => Some(1_763_649_000),
            Self::MonadNine => Some(1_773_930_600),
            Self::MonadTen => Some(1_788_359_400),
            Self::MonadNext => None,
        }
    }

    /// Retrieves the activation timestamp for this hardfork on testnet.
    pub const fn testnet_activation_timestamp(&self) -> Option<u64> {
        match self {
            Self::MonadEight => Some(1_763_562_600),
            Self::MonadNine => Some(1_773_153_000),
            Self::MonadTen => Some(1_786_545_000),
            Self::MonadNext => None,
        }
    }

    const fn from_mainnet_timestamp(timestamp: u64) -> Self {
        if timestamp >= 1_788_359_400 {
            Self::MonadTen
        } else {
            Self::from_timestamp(timestamp, 1_773_930_600)
        }
    }

    const fn from_testnet_timestamp(timestamp: u64) -> Self {
        if timestamp >= 1_786_545_000 {
            Self::MonadTen
        } else {
            Self::from_timestamp(timestamp, 1_773_153_000)
        }
    }

    const fn from_timestamp(timestamp: u64, monad_nine_timestamp: u64) -> Self {
        if timestamp >= monad_nine_timestamp {
            Self::MonadNine
        } else {
            // Older Monad revisions are represented as MonadEight.
            Self::MonadEight
        }
    }
}

impl From<MonadHardfork> for SpecId {
    fn from(spec: MonadHardfork) -> Self {
        spec.into_eth_spec()
    }
}

/// Error returned when parsing an unknown Monad hardfork name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownMonadHardfork;

impl fmt::Display for UnknownMonadHardfork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown Monad hardfork")
    }
}

impl core::error::Error for UnknownMonadHardfork {}

impl FromStr for MonadHardfork {
    type Err = UnknownMonadHardfork;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            s if s.eq_ignore_ascii_case(name::MONAD_EIGHT) => Ok(Self::MonadEight),
            s if s.eq_ignore_ascii_case(name::MONAD_NINE) => Ok(Self::MonadNine),
            s if s.eq_ignore_ascii_case(name::MONAD_TEN) => Ok(Self::MonadTen),
            s if s.eq_ignore_ascii_case(name::MONAD_NEXT) => Ok(Self::MonadNext),
            _ => Err(UnknownMonadHardfork),
        }
    }
}

impl From<MonadHardfork> for &'static str {
    fn from(spec_id: MonadHardfork) -> Self {
        match spec_id {
            MonadHardfork::MonadEight => name::MONAD_EIGHT,
            MonadHardfork::MonadNine => name::MONAD_NINE,
            MonadHardfork::MonadTen => name::MONAD_TEN,
            MonadHardfork::MonadNext => name::MONAD_NEXT,
        }
    }
}

impl fmt::Display for MonadHardfork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name: &'static str = (*self).into();
        f.write_str(name)
    }
}

/// String identifiers for Monad hardforks.
pub mod name {
    /// Mainnet launch spec name.
    pub const MONAD_EIGHT: &str = "MonadEight";
    /// MIP-3, MIP-4 and MIP-5 spec name.
    pub const MONAD_NINE: &str = "MonadNine";
    /// MIP-8 spec name.
    pub const MONAD_TEN: &str = "MonadTen";
    /// Development spec name.
    pub const MONAD_NEXT: &str = "MonadNext";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eth_spec_mapping_matches_monad_revm() {
        for spec in MonadHardfork::ALL {
            let expected: revm::primitives::hardfork::SpecId =
                monad_revm::MonadHardfork::from_str(spec.into()).unwrap().into_eth_spec();
            assert_eq!(spec.into_eth_spec() as u8, expected as u8, "{spec}");
            assert_eq!(
                spec as u8,
                monad_revm::MonadHardfork::from_str(spec.into()).unwrap() as u8,
                "{spec}"
            );
        }
    }

    #[test]
    fn default_and_names() {
        assert_eq!(MonadHardfork::default(), MonadHardfork::MonadTen);
        assert_eq!("monadnine".parse::<MonadHardfork>().unwrap(), MonadHardfork::MonadNine);
        assert!("Unknown".parse::<MonadHardfork>().is_err());
        assert_eq!(MonadHardfork::MonadNext.to_string(), "MonadNext");
    }

    #[test]
    fn timestamps_match_monad_revm() {
        let points = [
            (MONAD_MAINNET_CHAIN_ID, 1_763_648_999),
            (MONAD_MAINNET_CHAIN_ID, 1_773_930_599),
            (MONAD_MAINNET_CHAIN_ID, 1_773_930_600),
            (MONAD_MAINNET_CHAIN_ID, 1_788_359_399),
            (MONAD_MAINNET_CHAIN_ID, 1_788_359_400),
            (MONAD_TESTNET_CHAIN_ID, 1_773_152_999),
            (MONAD_TESTNET_CHAIN_ID, 1_773_153_000),
            (MONAD_TESTNET_CHAIN_ID, 1_786_544_999),
            (MONAD_TESTNET_CHAIN_ID, 1_786_545_000),
            (1, 1_773_153_000),
        ];
        for (chain, timestamp) in points {
            let expected = monad_revm::MonadHardfork::from_chain_and_timestamp(chain, timestamp)
                .map(|spec| spec as u8);
            let actual =
                MonadHardfork::from_chain_and_timestamp(chain, timestamp).map(|spec| spec as u8);
            assert_eq!(actual, expected, "chain {chain} timestamp {timestamp}");
        }
        for spec in MonadHardfork::ALL {
            let theirs = monad_revm::MonadHardfork::from_str(spec.into()).unwrap();
            assert_eq!(spec.mainnet_activation_timestamp(), theirs.mainnet_activation_timestamp());
            assert_eq!(spec.testnet_activation_timestamp(), theirs.testnet_activation_timestamp());
        }
    }
}
