//! L1 block info: the L1 fee inputs read from the `L1Block` predeploy.

use alloy_primitives::U256;
use base_common_consensus::Predeploys;
use base_common_genesis::BaseUpgrade;
use base_common_l1_fees::L1FeeParams;
use evm2::{DatabaseError, Evm};

use crate::{BaseEvmTypes, BaseSpecId};

/// L1 block info used to price the L1 data fee and the operator fee.
///
/// The values are read from the `L1Block` predeploy storage, which the L1 attributes deposit at
/// the start of every L2 block updates. They are cached per L2 block in
/// [`BaseEvmExt::l1_block_info`](crate::BaseEvmExt::l1_block_info) and reloaded when the block
/// number changes.
///
/// Field-for-field port of `base_common_evm::L1BlockInfo`. The fee math is delegated to the
/// engine-neutral [`L1FeeParams`], which the revm integration uses too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct L1BlockInfo {
    /// The L2 block number this info was loaded for. If it differs from the block being executed,
    /// the info is stale and is reloaded from state.
    pub l2_block: Option<U256>,
    /// The base fee of the L1 origin block.
    pub l1_base_fee: U256,
    /// The L1 fee overhead. `None` once Ecotone is active, unless the Ecotone scalars are empty.
    pub l1_fee_overhead: Option<U256>,
    /// The L1 base fee scalar.
    pub l1_base_fee_scalar: U256,
    /// The L1 blob base fee. `None` before Ecotone.
    pub l1_blob_base_fee: Option<U256>,
    /// The L1 blob base fee scalar. `None` before Ecotone.
    pub l1_blob_base_fee_scalar: Option<U256>,
    /// The operator fee scalar. `None` before Isthmus.
    pub operator_fee_scalar: Option<U256>,
    /// The operator fee constant. `None` before Isthmus.
    pub operator_fee_constant: Option<U256>,
    /// The DA footprint gas scalar. `None` before Jovian.
    pub da_footprint_gas_scalar: Option<u16>,
    /// True if Ecotone is active but the L1 fee scalars have not been set yet.
    pub empty_ecotone_scalars: bool,
    /// The L1 data fee of the transaction currently executing, cached between validation and
    /// settlement.
    pub tx_l1_cost: Option<U256>,
}

impl L1BlockInfo {
    /// Byte offset of the 4-byte `baseFeeScalar` in the Ecotone fee-scalars slot.
    pub const BASE_FEE_SCALAR_OFFSET: usize = 16;
    /// Byte offset of the 4-byte `blobBaseFeeScalar` in the Ecotone fee-scalars slot.
    pub const BLOB_BASE_FEE_SCALAR_OFFSET: usize = 20;
    /// Byte offset of the 4-byte `operatorFeeScalar` in the operator-fee slot.
    pub const OPERATOR_FEE_SCALAR_OFFSET: usize = 20;
    /// Byte offset of the 8-byte `operatorFeeConstant` in the operator-fee slot.
    pub const OPERATOR_FEE_CONSTANT_OFFSET: usize = 24;
    /// Byte offset of the 2-byte `daFootprintGasScalar` in its slot.
    pub const DA_FOOTPRINT_GAS_SCALAR_OFFSET: usize = 18;
    /// The L1 base fee storage slot.
    pub const L1_BASE_FEE_SLOT: U256 = U256::from_limbs([1, 0, 0, 0]);
    /// The L1 overhead storage slot.
    pub const L1_OVERHEAD_SLOT: U256 = U256::from_limbs([5, 0, 0, 0]);
    /// The L1 scalar storage slot.
    pub const L1_SCALAR_SLOT: U256 = U256::from_limbs([6, 0, 0, 0]);
    /// The Ecotone L1 blob base fee storage slot.
    pub const ECOTONE_L1_BLOB_BASE_FEE_SLOT: U256 = U256::from_limbs([7, 0, 0, 0]);
    /// The Ecotone fee-scalars storage slot.
    pub const ECOTONE_L1_FEE_SCALARS_SLOT: U256 = U256::from_limbs([3, 0, 0, 0]);
    /// The Isthmus operator-fee storage slot.
    pub const OPERATOR_FEE_SCALARS_SLOT: U256 = U256::from_limbs([8, 0, 0, 0]);
    /// The Jovian DA-footprint gas scalar storage slot.
    pub const DA_FOOTPRINT_GAS_SCALAR_SLOT: U256 = U256::from_limbs([8, 0, 0, 0]);
    /// An empty 64-bit set of scalar values.
    pub const EMPTY_SCALARS: [u8; 8] = [0u8; 8];

    /// Loads the L1 block info for `l2_block` at `spec` through `storage`, a reader for the
    /// `L1Block` predeploy's storage slots.
    ///
    /// Mirrors `base_common_evm::L1BlockInfo::try_fetch` slot for slot.
    pub fn try_fetch_with<E>(
        mut storage: impl FnMut(U256) -> Result<U256, E>,
        l2_block: U256,
        spec: BaseSpecId,
    ) -> Result<Self, E> {
        let mut out = Self {
            l2_block: Some(l2_block),
            l1_base_fee: storage(Self::L1_BASE_FEE_SLOT)?,
            ..Default::default()
        };

        if !spec.is_enabled_in(BaseUpgrade::Ecotone) {
            out.l1_base_fee_scalar = storage(Self::L1_SCALAR_SLOT)?;
            out.l1_fee_overhead = Some(storage(Self::L1_OVERHEAD_SLOT)?);
            return Ok(out);
        }

        // Post-Ecotone.
        out.l1_blob_base_fee = Some(storage(Self::ECOTONE_L1_BLOB_BASE_FEE_SLOT)?);
        let scalars = storage(Self::ECOTONE_L1_FEE_SCALARS_SLOT)?.to_be_bytes::<32>();
        out.l1_base_fee_scalar = U256::from_be_slice(
            &scalars[Self::BASE_FEE_SCALAR_OFFSET..Self::BASE_FEE_SCALAR_OFFSET + 4],
        );
        let blob_base_fee_scalar = U256::from_be_slice(
            &scalars[Self::BLOB_BASE_FEE_SCALAR_OFFSET..Self::BLOB_BASE_FEE_SCALAR_OFFSET + 4],
        );
        out.l1_blob_base_fee_scalar = Some(blob_base_fee_scalar);
        // With empty Ecotone scalars the Bedrock cost function applies, which needs the overhead.
        out.empty_ecotone_scalars = blob_base_fee_scalar.is_zero()
            && scalars[Self::BASE_FEE_SCALAR_OFFSET..Self::BLOB_BASE_FEE_SCALAR_OFFSET + 4]
                == Self::EMPTY_SCALARS;
        out.l1_fee_overhead =
            if out.empty_ecotone_scalars { Some(storage(Self::L1_OVERHEAD_SLOT)?) } else { None };

        // Post-Isthmus.
        if spec.is_enabled_in(BaseUpgrade::Isthmus) {
            let operator = storage(Self::OPERATOR_FEE_SCALARS_SLOT)?.to_be_bytes::<32>();
            out.operator_fee_scalar = Some(U256::from_be_slice(
                &operator[Self::OPERATOR_FEE_SCALAR_OFFSET..Self::OPERATOR_FEE_SCALAR_OFFSET + 4],
            ));
            out.operator_fee_constant = Some(U256::from_be_slice(
                &operator
                    [Self::OPERATOR_FEE_CONSTANT_OFFSET..Self::OPERATOR_FEE_CONSTANT_OFFSET + 8],
            ));
        }

        // Post-Jovian.
        if spec.is_enabled_in(BaseUpgrade::Jovian) {
            let slot = storage(Self::DA_FOOTPRINT_GAS_SCALAR_SLOT)?.to_be_bytes::<32>();
            out.da_footprint_gas_scalar = Some(u16::from_be_bytes([
                slot[Self::DA_FOOTPRINT_GAS_SCALAR_OFFSET],
                slot[Self::DA_FOOTPRINT_GAS_SCALAR_OFFSET + 1],
            ]));
        }

        Ok(out)
    }

    /// Loads the L1 block info for `l2_block` from the committed state of `evm`.
    ///
    /// Reads the accepted overlay and the backing database, ignoring the in-flight transaction,
    /// like the reference reads its journal's database.
    pub fn try_fetch(
        evm: &mut Evm<'_, BaseEvmTypes>,
        l2_block: U256,
        spec: BaseSpecId,
    ) -> Result<Self, DatabaseError> {
        let state = evm.state_mut();
        Self::try_fetch_with(
            |slot| state.read_committed_storage(&Predeploys::L1_BLOCK_INFO, &slot),
            l2_block,
            spec,
        )
    }

    /// Builds L1 block info for `l2_block` from engine-neutral fee parameters.
    pub fn from_params(params: L1FeeParams, l2_block: Option<U256>) -> Self {
        Self {
            l2_block,
            l1_base_fee: params.l1_base_fee,
            l1_fee_overhead: params.l1_fee_overhead,
            l1_base_fee_scalar: params.l1_base_fee_scalar,
            l1_blob_base_fee: params.l1_blob_base_fee,
            l1_blob_base_fee_scalar: params.l1_blob_base_fee_scalar,
            operator_fee_scalar: params.operator_fee_scalar,
            operator_fee_constant: params.operator_fee_constant,
            da_footprint_gas_scalar: params
                .da_footprint_gas_scalar
                .map(|scalar| scalar.saturating_to::<u16>()),
            empty_ecotone_scalars: params.empty_ecotone_scalars,
            tx_l1_cost: None,
        }
    }

    /// Returns the engine-neutral fee parameters of this block info.
    pub fn params(&self) -> L1FeeParams {
        L1FeeParams {
            l1_base_fee: self.l1_base_fee,
            l1_fee_overhead: self.l1_fee_overhead,
            l1_base_fee_scalar: self.l1_base_fee_scalar,
            l1_blob_base_fee: self.l1_blob_base_fee,
            l1_blob_base_fee_scalar: self.l1_blob_base_fee_scalar,
            operator_fee_scalar: self.operator_fee_scalar,
            operator_fee_constant: self.operator_fee_constant,
            empty_ecotone_scalars: self.empty_ecotone_scalars,
            da_footprint_gas_scalar: self.da_footprint_gas_scalar.map(U256::from),
        }
    }

    /// Clears the cached L1 data fee of the current transaction.
    pub const fn clear_tx_l1_cost(&mut self) {
        self.tx_l1_cost = None;
    }

    /// Returns the L1 data fee for `input`, caching it for the current transaction.
    ///
    /// Empty input and deposits are exempt. Mirrors
    /// `base_common_evm::L1BlockInfo::calculate_tx_l1_cost`, including its cache.
    pub fn calculate_tx_l1_cost(&mut self, input: &[u8], spec: BaseSpecId) -> U256 {
        if let Some(cost) = self.tx_l1_cost {
            return cost;
        }
        if input.is_empty() || input.first() == Some(&0x7E) {
            return U256::ZERO;
        }
        let cost = self.params().calculate_tx_l1_cost(input, spec.upgrade());
        self.tx_l1_cost = Some(cost);
        cost
    }

    /// Returns the operator fee for `input` over `gas`. Empty input and deposits are exempt; zero
    /// before Isthmus.
    pub fn operator_fee_charge(&self, input: &[u8], gas: U256, spec: BaseSpecId) -> U256 {
        if !spec.is_enabled_in(BaseUpgrade::Isthmus) {
            return U256::ZERO;
        }
        self.params().operator_fee_charge(input, gas, spec.upgrade())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Bytes, U256, bytes};
    use base_common_evm::L1BlockInfo as RevmL1BlockInfo;
    use revm::database::InMemoryDB;

    const UPGRADES: [BaseUpgrade; 6] = [
        BaseUpgrade::Regolith,
        BaseUpgrade::Ecotone,
        BaseUpgrade::Fjord,
        BaseUpgrade::Isthmus,
        BaseUpgrade::Jovian,
        BaseUpgrade::Azul,
    ];

    fn slots(empty_scalars: bool) -> Vec<(U256, U256)> {
        let mut scalars = [0u8; 32];
        let mut operator = [0u8; 32];
        if !empty_scalars {
            scalars[16..20].copy_from_slice(&1_368u32.to_be_bytes());
            scalars[20..24].copy_from_slice(&810_949u32.to_be_bytes());
            operator[18..20].copy_from_slice(&400u16.to_be_bytes());
            operator[20..24].copy_from_slice(&5_000u32.to_be_bytes());
            operator[24..32].copy_from_slice(&123_456u64.to_be_bytes());
        }
        vec![
            (L1BlockInfo::L1_BASE_FEE_SLOT, U256::from(7_000_000_000u64)),
            (L1BlockInfo::ECOTONE_L1_FEE_SCALARS_SLOT, U256::from_be_bytes(scalars)),
            (L1BlockInfo::L1_OVERHEAD_SLOT, U256::from(188)),
            (L1BlockInfo::L1_SCALAR_SLOT, U256::from(684_000)),
            (L1BlockInfo::ECOTONE_L1_BLOB_BASE_FEE_SLOT, U256::from(1)),
            (L1BlockInfo::OPERATOR_FEE_SCALARS_SLOT, U256::from_be_bytes(operator)),
        ]
    }

    /// Loads the same `L1Block` storage through the revm reference and this port, then compares
    /// every field and the derived fees.
    #[test]
    fn fetch_and_fees_match_revm_reference() {
        let input: Bytes = bytes!(
            "02f86b8221058203e8830f4240830f42408252089400000000000000000000000000000000000000008080c0"
        );
        for empty_scalars in [false, true] {
            let slots = slots(empty_scalars);
            let mut db = InMemoryDB::default();
            for (slot, value) in &slots {
                db.insert_account_storage(Predeploys::L1_BLOCK_INFO, *slot, *value).unwrap();
            }
            for upgrade in UPGRADES {
                let block = U256::from(42);
                let mut revm_info = RevmL1BlockInfo::try_fetch(
                    &mut db,
                    block,
                    base_common_evm::BaseSpecId::new(upgrade),
                )
                .unwrap();
                let mut info = L1BlockInfo::try_fetch_with(
                    |slot| {
                        Ok::<_, ()>(
                            slots
                                .iter()
                                .find(|(s, _)| *s == slot)
                                .map(|(_, v)| *v)
                                .unwrap_or_default(),
                        )
                    },
                    block,
                    BaseSpecId::new(upgrade),
                )
                .unwrap();

                assert_eq!(info.l2_block, revm_info.l2_block);
                assert_eq!(info.l1_base_fee, revm_info.l1_base_fee);
                assert_eq!(info.l1_fee_overhead, revm_info.l1_fee_overhead);
                assert_eq!(info.l1_base_fee_scalar, revm_info.l1_base_fee_scalar);
                assert_eq!(info.l1_blob_base_fee, revm_info.l1_blob_base_fee);
                assert_eq!(info.l1_blob_base_fee_scalar, revm_info.l1_blob_base_fee_scalar);
                assert_eq!(info.operator_fee_scalar, revm_info.operator_fee_scalar);
                assert_eq!(info.operator_fee_constant, revm_info.operator_fee_constant);
                assert_eq!(info.da_footprint_gas_scalar, revm_info.da_footprint_gas_scalar);
                assert_eq!(info.empty_ecotone_scalars, revm_info.empty_ecotone_scalars);

                let spec = BaseSpecId::new(upgrade);
                let revm_spec = base_common_evm::BaseSpecId::new(upgrade);
                assert_eq!(
                    info.calculate_tx_l1_cost(&input, spec),
                    revm_info.calculate_tx_l1_cost(&input, revm_spec),
                    "{upgrade:?}"
                );
                let gas = U256::from(77_777);
                let revm_operator = if revm_spec.is_enabled_in(BaseUpgrade::Isthmus) {
                    revm_info.operator_fee_charge(&input, gas, revm_spec)
                } else {
                    U256::ZERO
                };
                assert_eq!(info.operator_fee_charge(&input, gas, spec), revm_operator);
            }
        }
    }

    #[test]
    fn l1_cost_is_cached_until_cleared() {
        let mut info = L1BlockInfo {
            l1_base_fee: U256::from(1_000),
            l1_base_fee_scalar: U256::from(1_000),
            l1_fee_overhead: Some(U256::from(1_000)),
            ..Default::default()
        };
        let spec = BaseSpecId::new(BaseUpgrade::Regolith);
        // Regolith: (16 * 3 + 1000) * 1000 * 1000 / 1e6 = 1048 (vector from base-common-evm).
        assert_eq!(info.calculate_tx_l1_cost(&bytes!("FACADE"), spec), U256::from(1048));
        // The cached value wins even for different input.
        assert_eq!(info.calculate_tx_l1_cost(&bytes!("FACADEFACADE"), spec), U256::from(1048));
        info.clear_tx_l1_cost();
        assert_ne!(info.calculate_tx_l1_cost(&bytes!("FACADEFACADE"), spec), U256::from(1048));
        // Deposits and empty input are exempt and never cached.
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&bytes!("7EFACADE"), spec), U256::ZERO);
        assert_eq!(info.tx_l1_cost, None);
    }
}
