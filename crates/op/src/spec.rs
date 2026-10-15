//! OP Stack hardfork identifiers.
//!
//! Port of `op-revm/src/spec.rs`.

use core::{fmt, str::FromStr};
use evm2::SpecId;

/// OP Stack hardfork identifier.
///
/// Discriminants, ordering and the mapping to the inherited Ethereum [`SpecId`] match op-revm's
/// `OpSpecId` exactly.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub enum OpSpecId {
    /// Bedrock spec id.
    BEDROCK = 100,
    /// Regolith spec id.
    REGOLITH,
    /// Canyon spec id.
    CANYON,
    /// Ecotone spec id.
    ECOTONE,
    /// Fjord spec id.
    FJORD,
    /// Granite spec id.
    GRANITE,
    /// Holocene spec id.
    HOLOCENE,
    /// Isthmus spec id.
    ISTHMUS,
    /// Jovian spec id.
    #[default]
    JOVIAN,
    /// Karst spec id.
    KARST,
    /// Lagoon spec id.
    LAGOON,
}

impl OpSpecId {
    /// All OP hardforks in activation order.
    pub const ALL: [Self; 11] = [
        Self::BEDROCK,
        Self::REGOLITH,
        Self::CANYON,
        Self::ECOTONE,
        Self::FJORD,
        Self::GRANITE,
        Self::HOLOCENE,
        Self::ISTHMUS,
        Self::JOVIAN,
        Self::KARST,
        Self::LAGOON,
    ];

    /// Converts the [`OpSpecId`] into the inherited Ethereum [`SpecId`].
    pub const fn into_eth_spec(self) -> SpecId {
        match self {
            Self::BEDROCK | Self::REGOLITH => SpecId::MERGE,
            Self::CANYON => SpecId::SHANGHAI,
            Self::ECOTONE | Self::FJORD | Self::GRANITE | Self::HOLOCENE => SpecId::CANCUN,
            Self::ISTHMUS | Self::JOVIAN => SpecId::PRAGUE,
            Self::KARST | Self::LAGOON => SpecId::OSAKA,
        }
    }

    /// Returns `true` if `other` is active when `self` is the current hardfork.
    ///
    /// Same argument order as op-revm: `spec.is_enabled_in(OpSpecId::REGOLITH)` reads "Regolith
    /// rules are enabled in `spec`".
    pub const fn is_enabled_in(self, other: Self) -> bool {
        other as u8 <= self as u8
    }

    /// Returns the hardfork name as used by op-revm.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BEDROCK => name::BEDROCK,
            Self::REGOLITH => name::REGOLITH,
            Self::CANYON => name::CANYON,
            Self::ECOTONE => name::ECOTONE,
            Self::FJORD => name::FJORD,
            Self::GRANITE => name::GRANITE,
            Self::HOLOCENE => name::HOLOCENE,
            Self::ISTHMUS => name::ISTHMUS,
            Self::JOVIAN => name::JOVIAN,
            Self::KARST => name::KARST,
            Self::LAGOON => name::LAGOON,
        }
    }
}

impl From<OpSpecId> for SpecId {
    fn from(spec: OpSpecId) -> Self {
        spec.into_eth_spec()
    }
}

impl From<OpSpecId> for &'static str {
    fn from(spec: OpSpecId) -> Self {
        spec.as_str()
    }
}

impl fmt::Display for OpSpecId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown OP hardfork name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, thiserror::Error)]
#[error("unknown OP hardfork")]
pub struct UnknownOpHardfork;

impl FromStr for OpSpecId {
    type Err = UnknownOpHardfork;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|spec| spec.as_str() == s).ok_or(UnknownOpHardfork)
    }
}

/// String identifiers for OP hardforks, matching op-revm.
pub mod name {
    /// Bedrock spec name.
    pub const BEDROCK: &str = "Bedrock";
    /// Regolith spec name.
    pub const REGOLITH: &str = "Regolith";
    /// Canyon spec name.
    pub const CANYON: &str = "Canyon";
    /// Ecotone spec name.
    pub const ECOTONE: &str = "Ecotone";
    /// Fjord spec name.
    pub const FJORD: &str = "Fjord";
    /// Granite spec name.
    pub const GRANITE: &str = "Granite";
    /// Holocene spec name.
    pub const HOLOCENE: &str = "Holocene";
    /// Isthmus spec name.
    pub const ISTHMUS: &str = "Isthmus";
    /// Jovian spec name.
    pub const JOVIAN: &str = "Jovian";
    /// Karst spec name.
    pub const KARST: &str = "Karst";
    /// Lagoon spec name.
    pub const LAGOON: &str = "Lagoon";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eth_spec_mapping_matches_op_revm() {
        let expected = [
            (OpSpecId::BEDROCK, SpecId::MERGE),
            (OpSpecId::REGOLITH, SpecId::MERGE),
            (OpSpecId::CANYON, SpecId::SHANGHAI),
            (OpSpecId::ECOTONE, SpecId::CANCUN),
            (OpSpecId::FJORD, SpecId::CANCUN),
            (OpSpecId::GRANITE, SpecId::CANCUN),
            (OpSpecId::HOLOCENE, SpecId::CANCUN),
            (OpSpecId::ISTHMUS, SpecId::PRAGUE),
            (OpSpecId::JOVIAN, SpecId::PRAGUE),
            (OpSpecId::KARST, SpecId::OSAKA),
            (OpSpecId::LAGOON, SpecId::OSAKA),
        ];
        for (op, eth) in expected {
            assert_eq!(SpecId::from(op), eth, "{op:?}");
        }
        assert_eq!(OpSpecId::default(), OpSpecId::JOVIAN);
        assert_eq!(OpSpecId::BEDROCK as u8, 100);
        assert_eq!(OpSpecId::LAGOON as u8, 110);
    }

    #[test]
    fn enabled_in_and_names_round_trip() {
        for pair in OpSpecId::ALL.windows(2) {
            assert!(pair[1].is_enabled_in(pair[0]));
            assert!(!pair[0].is_enabled_in(pair[1]));
            assert!(pair[1].into_eth_spec() >= pair[0].into_eth_spec());
        }
        for spec in OpSpecId::ALL {
            assert_eq!(spec.as_str().parse::<OpSpecId>(), Ok(spec));
        }
        assert_eq!("Isthmus".parse::<OpSpecId>(), Ok(OpSpecId::ISTHMUS));
        assert_eq!("isthmus".parse::<OpSpecId>(), Err(UnknownOpHardfork));
    }
}
