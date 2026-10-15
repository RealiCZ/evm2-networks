//! Base's evm2 execution type family.

use alloy_primitives::U256;
use base_common_l1_fees::L1FeeParams;
use evm2::{
    EvmConfig, EvmConfigSelector, EvmTypesHost, ExecutionConfig, OpcodeConfig, SpecId, Version,
};

use crate::{BaseSpecId, BaseTxEnvelope, L1BlockInfo};

/// Base's evm2 instance.
pub type BaseEvm<'a> = evm2::Evm<'a, BaseEvmTypes>;

/// Base's evm2 execution type family.
///
/// Wires the Base transaction envelope ([`BaseTxEnvelope`], including deposits), the Base upgrade
/// schedule ([`BaseSpecId`]), the cached L1 block info ([`BaseEvmExt`]), and the L1-fee and
/// deposit result data ([`BaseTxResultExt`]) into evm2's [`EvmTypesHost`].
#[derive(Clone, Copy, Debug)]
pub struct BaseEvmTypes;

impl EvmTypesHost for BaseEvmTypes {
    type ConfigSelector = BaseConfigSelector;
    type SpecId = BaseSpecId;
    type Tx = BaseTxEnvelope;
    type EvmExt = BaseEvmExt;
    type MessageExt = ();
    type MessageResultExt = ();
    type TxEnvExt = ();
    type TxResultExt = BaseTxResultExt;
    type BlockEnvExt = BaseBlockEnvExt;
    type Host<'a> = evm2::Evm<'a, Self>;
}

/// Base opcode configuration over an inherited Ethereum specification.
///
/// Base does not change any opcode or opcode gas cost relative to the Ethereum spec its upgrade
/// maps to (`base_common_evm` builds `EthInstructions::new_mainnet_with_spec`), so this is the
/// stock evm2 table. It exists so Base has its own dispatch-table type to extend.
#[derive(Clone, Copy, Debug)]
pub struct BaseConfig<const BASE_SPEC_ID: u32>(());

impl<const BASE_SPEC_ID: u32> EvmConfig<BaseEvmTypes> for BaseConfig<BASE_SPEC_ID> {
    const BASE_SPEC_ID: SpecId =
        SpecId::try_from_u32(BASE_SPEC_ID).expect("invalid Base base spec id");
    const OPCODE_CONFIG: &'static OpcodeConfig<BaseEvmTypes> =
        &OpcodeConfig::<BaseEvmTypes>::base::<Self>();
}

/// Selects Base's opcode table for the Ethereum spec a Base upgrade maps to.
#[derive(Clone, Copy, Debug)]
pub struct BaseConfigSelector;

impl EvmConfigSelector<BaseEvmTypes> for BaseConfigSelector {
    type Config<const BASE_SPEC_ID: u32, const CUSTOM_SPEC_ID: u32> = BaseConfig<BASE_SPEC_ID>;

    fn execution_config(spec_id: BaseSpecId) -> ExecutionConfig<BaseEvmTypes> {
        let spec_id = SpecId::from(spec_id);
        evm2::spec_to_generic!(
            spec_id,
            |SPEC_ID| ExecutionConfig::for_config::<BaseConfig<SPEC_ID>>()
        )
    }
}

/// Block-environment extension for Base.
///
/// By default the L1 fee inputs are read from the `L1Block` predeploy, exactly like
/// `base_common_evm`. Setting [`Self::l1_fee_params`] overrides them for the block, which is
/// useful for callers that already decoded the L1 attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BaseBlockEnvExt {
    /// L1 fee inputs that override the `L1Block` predeploy storage, if set.
    pub l1_fee_params: Option<L1FeeParams>,
}

/// EVM instance-wide extension state for Base.
///
/// Holds the per-block L1 block info cache (the reference's `L1BlockInfo` chain context) and the
/// fees charged for the transaction currently executing, so settlement refunds and distributes
/// against the exact amounts charged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BaseEvmExt {
    /// Cached L1 block info, reloaded when the block number changes.
    pub l1_block_info: L1BlockInfo,
    /// Operator fee charged upfront, on the gas limit, for the transaction currently executing.
    pub operator_fee_charged: U256,
}

impl BaseEvmExt {
    /// Creates the extension with preloaded L1 block info.
    pub const fn with_l1_block_info(l1_block_info: L1BlockInfo) -> Self {
        Self { l1_block_info, operator_fee_charged: U256::ZERO }
    }

    /// Clears the cached L1 data fee of the current transaction.
    pub const fn clear_tx_l1_cost(&mut self) {
        self.l1_block_info.clear_tx_l1_cost();
    }
}

/// Transaction-result extension data for Base transactions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BaseTxResultExt {
    /// L1 data fee credited to the L1 fee vault. Zero for deposits.
    pub l1_fee: U256,
    /// Operator fee credited to the operator fee vault, on the gas used. Zero for deposits and
    /// before Isthmus.
    pub operator_fee: U256,
    /// Whether this is a failed deposit.
    ///
    /// A failed deposit has all its execution reverted, keeps only the mint and the sender nonce
    /// bump, and consumes its full gas limit. It is the reference's
    /// `BaseHaltReason::FailedDeposit`.
    pub failed_deposit: bool,
    /// For deposits, the sender nonce before the deposit, as recorded in Canyon deposit receipts.
    pub deposit_nonce: Option<u64>,
}

/// Returns the evm2 [`Version`] for `spec` on chain `chain_id`.
///
/// Base uses the stock version of the Ethereum spec its upgrade maps to; only the chain id is
/// set.
pub fn base_version(spec: BaseSpecId, chain_id: u64) -> Version {
    let mut version = Version::new(spec.into());
    version.chain_id = chain_id;
    version
}

/// Returns the execution config for `spec` with the runtime `version`.
pub fn base_execution_config(spec: BaseSpecId, version: Version) -> ExecutionConfig<BaseEvmTypes> {
    ExecutionConfig::for_spec_and_version(spec, version)
}
