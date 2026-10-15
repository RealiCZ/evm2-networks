//! Base upgrade schedule for the evm2 execution type family.

use base_common_genesis::BaseUpgrade;
use evm2::SpecId;

/// EVM-facing Base spec id.
///
/// Wraps the canonical Base network upgrade ([`BaseUpgrade`]) and maps it, via
/// [`From<BaseSpecId>`] for [`SpecId`], to the evm2 spec whose feature set and gas schedule govern
/// execution at that upgrade. Used as [`BaseEvmTypes::SpecId`](crate::BaseEvmTypes), it keeps the
/// active Base upgrade recoverable at runtime (`Evm::config_spec_id`) for fork-dependent fee and
/// precompile logic, even when several upgrades share one evm2 spec (for example Ecotone and Fjord
/// both map to [`SpecId::CANCUN`]).
///
/// Mirrors `base_common_evm::BaseSpecId`.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseSpecId(BaseUpgrade);

impl BaseSpecId {
    /// Creates a Base spec id for the given Base upgrade.
    pub const fn new(upgrade: BaseUpgrade) -> Self {
        Self(upgrade)
    }

    /// Returns the wrapped Base upgrade.
    pub const fn upgrade(self) -> BaseUpgrade {
        self.0
    }

    /// Returns whether `other` is active at this spec.
    ///
    /// Base upgrades are ordered by discriminant, matching `base_common_evm::BaseSpecId`.
    pub const fn is_enabled_in(self, other: BaseUpgrade) -> bool {
        other as u8 <= self.0 as u8
    }

    /// Returns the evm2 spec that governs execution at this Base upgrade.
    ///
    /// Mirrors `base_common_chains::BaseUpgradeExt::into_eth_spec`. That mapping is duplicated
    /// here rather than reused because it targets revm's `SpecId` and pulls in revm.
    pub const fn into_eth_spec(self) -> SpecId {
        match self.0 {
            BaseUpgrade::Bedrock | BaseUpgrade::Regolith => SpecId::MERGE,
            BaseUpgrade::Canyon | BaseUpgrade::Delta => SpecId::SHANGHAI,
            BaseUpgrade::Ecotone
            | BaseUpgrade::Fjord
            | BaseUpgrade::Granite
            | BaseUpgrade::Holocene
            | BaseUpgrade::PectraBlobSchedule => SpecId::CANCUN,
            BaseUpgrade::Isthmus | BaseUpgrade::Jovian => SpecId::PRAGUE,
            // Azul, Beryl, Cobalt, Denim, Zenith, and newer upgrades inherit the latest known
            // Ethereum spec until explicitly mapped.
            _ => SpecId::OSAKA,
        }
    }
}

impl From<BaseUpgrade> for BaseSpecId {
    fn from(upgrade: BaseUpgrade) -> Self {
        Self(upgrade)
    }
}

impl From<BaseSpecId> for BaseUpgrade {
    fn from(spec: BaseSpecId) -> Self {
        spec.0
    }
}

impl From<BaseSpecId> for SpecId {
    fn from(spec: BaseSpecId) -> Self {
        spec.into_eth_spec()
    }
}

impl core::fmt::Display for BaseSpecId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl core::str::FromStr for BaseSpecId {
    type Err = <BaseUpgrade as core::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<BaseUpgrade>().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_common_chains::BaseUpgradeExt;
    use revm::primitives::hardfork::SpecId as RevmSpecId;

    const ALL: [BaseUpgrade; 16] = [
        BaseUpgrade::Bedrock,
        BaseUpgrade::Regolith,
        BaseUpgrade::Canyon,
        BaseUpgrade::Delta,
        BaseUpgrade::Ecotone,
        BaseUpgrade::Fjord,
        BaseUpgrade::Granite,
        BaseUpgrade::Holocene,
        BaseUpgrade::PectraBlobSchedule,
        BaseUpgrade::Isthmus,
        BaseUpgrade::Jovian,
        BaseUpgrade::Azul,
        BaseUpgrade::Beryl,
        BaseUpgrade::Cobalt,
        BaseUpgrade::Denim,
        BaseUpgrade::Zenith,
    ];

    #[test]
    fn eth_spec_mapping_matches_revm_reference() {
        for upgrade in ALL {
            let expected = match upgrade.into_eth_spec() {
                RevmSpecId::MERGE => SpecId::MERGE,
                RevmSpecId::SHANGHAI => SpecId::SHANGHAI,
                RevmSpecId::CANCUN => SpecId::CANCUN,
                RevmSpecId::PRAGUE => SpecId::PRAGUE,
                RevmSpecId::OSAKA => SpecId::OSAKA,
                other => panic!("unexpected revm spec {other:?}"),
            };
            assert_eq!(
                BaseSpecId::new(upgrade).into_eth_spec(),
                expected,
                "evm2 spec mapping diverged from base-common-chains at {upgrade:?}",
            );
        }
    }

    #[test]
    fn enablement_matches_revm_reference() {
        for spec in ALL {
            for other in ALL {
                assert_eq!(
                    BaseSpecId::new(spec).is_enabled_in(other),
                    base_common_evm::BaseSpecId::new(spec).is_enabled_in(other),
                );
            }
        }
    }
}
