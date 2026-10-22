//! ArbOS versions, Arbitrum Nitro's hardfork identifiers.
//!
//! Nitro gates every protocol change on the ArbOS version stored in the ArbOS state and recorded in
//! each block header (see [`ArbHeaderInfo`](crate::ArbHeaderInfo)), rather than on block numbers
//! or timestamps. The Ethereum forks map onto ArbOS versions as in Nitro's `params/config.go`.

use core::{fmt, str::FromStr};
use evm2::SpecId;

/// ArbOS version.
///
/// Versions are compared numerically: a version enables every change of the versions below it.
/// The constants name the versions with execution changes this crate models.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArbOsVersion(u64);

impl ArbOsVersion {
    /// Poster fees go to the L1 pricer funds pool instead of the coinbase.
    pub const V2: Self = Self(2);
    /// `GASPRICE` returns the price actually paid.
    pub const V3: Self = Self(3);
    /// The infrastructure fee account receives part of the base fee (from version 5).
    pub const V4: Self = Self(4);
    /// Collects tips (the only version doing so before 60).
    pub const V9: Self = Self(9);
    /// Poster fees are added to the L1 fees available.
    pub const V10: Self = Self(10);
    /// Ethereum Shanghai.
    pub const V11: Self = Self(11);
    /// Ethereum Cancun (Atlas), without the KZG precompile.
    pub const V20: Self = Self(20);
    /// Stylus (Bianca): the Cancun precompiles, including KZG, and RIP-7212 P256.
    pub const V30: Self = Self(30);
    /// Stylus fixes.
    pub const V31: Self = Self(31);
    /// Stylus charging fixes.
    pub const V32: Self = Self(32);
    /// Ethereum Prague (Callisto), with the Cancun precompile set.
    pub const V40: Self = Self(40);
    /// Native token owners.
    pub const V41: Self = Self(41);
    /// Ethereum Osaka (Dia): the Osaka precompiles, a per-transaction gas limit and single-gas
    /// pricing constraints.
    pub const V50: Self = Self(50);
    /// Pricing constraint fixes.
    pub const V51: Self = Self(51);
    /// Configurable tip collection and multi-gas pricing constraints.
    pub const V60: Self = Self(60);
    /// Multi-gas refund fix.
    pub const V61: Self = Self(61);

    /// Latest ArbOS version modeled by this crate.
    pub const LATEST: Self = Self::V61;

    /// Creates an ArbOS version.
    pub const fn new(version: u64) -> Self {
        Self(version)
    }

    /// Returns the version number.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns `true` if `version` is active when `self` is the current version.
    pub const fn enables(self, version: Self) -> bool {
        self.0 >= version.0
    }

    /// Returns the Ethereum [`SpecId`] this version builds on.
    ///
    /// Nitro enables the Merge rules (`PREVRANDAO`) from the first ArbOS version.
    pub const fn into_eth_spec(self) -> SpecId {
        if self.enables(Self::V50) {
            SpecId::OSAKA
        } else if self.enables(Self::V40) {
            SpecId::PRAGUE
        } else if self.enables(Self::V20) {
            SpecId::CANCUN
        } else if self.enables(Self::V11) {
            SpecId::SHANGHAI
        } else {
            SpecId::MERGE
        }
    }
}

impl Default for ArbOsVersion {
    fn default() -> Self {
        Self::LATEST
    }
}

impl From<ArbOsVersion> for SpecId {
    fn from(version: ArbOsVersion) -> Self {
        version.into_eth_spec()
    }
}

impl From<u64> for ArbOsVersion {
    fn from(version: u64) -> Self {
        Self(version)
    }
}

impl From<ArbOsVersion> for u64 {
    fn from(version: ArbOsVersion) -> Self {
        version.0
    }
}

impl fmt::Display for ArbOsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ArbOS {}", self.0)
    }
}

/// Error returned when parsing an invalid ArbOS version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidArbOsVersion;

impl fmt::Display for InvalidArbOsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid ArbOS version")
    }
}

impl core::error::Error for InvalidArbOsVersion {}

impl FromStr for ArbOsVersion {
    type Err = InvalidArbOsVersion;

    /// Parses a version number with an optional `ArbOS` prefix, ignoring ASCII case and a `_`, `-`
    /// or space separator (`61`, `arbos61`, `ArbOS 61` and `arbos-61` are equivalent).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let number = match s.get(..5) {
            Some(prefix) if prefix.eq_ignore_ascii_case("arbos") => {
                let rest = &s[5..];
                rest.strip_prefix(['_', '-', ' ']).unwrap_or(rest)
            }
            _ => s,
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(InvalidArbOsVersion);
        }
        number.parse().map(Self).map_err(|_| InvalidArbOsVersion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eth_spec_boundaries() {
        for (version, spec) in [
            (1, SpecId::MERGE),
            (10, SpecId::MERGE),
            (11, SpecId::SHANGHAI),
            (19, SpecId::SHANGHAI),
            (20, SpecId::CANCUN),
            (32, SpecId::CANCUN),
            (39, SpecId::CANCUN),
            (40, SpecId::PRAGUE),
            (49, SpecId::PRAGUE),
            (50, SpecId::OSAKA),
            (61, SpecId::OSAKA),
        ] {
            assert_eq!(ArbOsVersion::new(version).into_eth_spec(), spec, "ArbOS {version}");
        }
    }

    #[test]
    fn parses_versions() {
        for s in ["61", "arbos61", "ArbOS 61", "ARBOS-61", "arbos_61"] {
            assert_eq!(s.parse::<ArbOsVersion>(), Ok(ArbOsVersion::V61), "{s}");
        }
        for s in ["", "arbos", "arbos 6 1", "v61", "-1", "61a", "arbos--61"] {
            assert!(s.parse::<ArbOsVersion>().is_err(), "{s}");
        }
        assert_eq!(ArbOsVersion::V40.to_string().parse::<ArbOsVersion>(), Ok(ArbOsVersion::V40));
        assert_eq!(ArbOsVersion::default(), ArbOsVersion::LATEST);
    }
}
