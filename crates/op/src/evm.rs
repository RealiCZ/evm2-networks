//! OP EVM type family, configuration and factory.

use crate::{L1BlockInfo, OpPrecompiles, OpSpecId, OpTxEnvelope, op_tx_registry};
use alloy_primitives::U256;
use evm2::{
    Evm, EvmConfig, EvmConfigSelector, EvmTypesHost, ExecutionConfig, OpcodeConfig, SpecId,
    TxResult, Version,
    env::BlockEnv,
    evm::DynDatabase,
    registry::{HandlerError, TxRegistry},
};

/// OP EVM instance.
pub type OpEvm<'a> = Evm<'a, OpEvmTypes>;

/// evm2 type family for OP Stack execution.
#[derive(Clone, Copy, Debug)]
pub struct OpEvmTypes;

impl EvmTypesHost for OpEvmTypes {
    type ConfigSelector = OpConfigSelector;
    type SpecId = OpSpecId;
    type Tx = OpTxEnvelope;
    type EvmExt = OpEvmExt;
    type MessageExt = ();
    type MessageResultExt = ();
    type TxEnvExt = ();
    type TxResultExt = OpTxResultExt;
    type BlockEnvExt = OpBlockEnvExt;
    type Host<'a> = Evm<'a, Self>;
}

/// OP opcode configuration over an inherited Ethereum specification.
///
/// op-revm uses the mainnet instruction set unchanged, so this is the base opcode configuration.
#[derive(Clone, Copy, Debug)]
pub struct OpConfig<const BASE_SPEC_ID: u32>(());

impl<const BASE_SPEC_ID: u32> EvmConfig<OpEvmTypes> for OpConfig<BASE_SPEC_ID> {
    const BASE_SPEC_ID: SpecId =
        SpecId::try_from_u32(BASE_SPEC_ID).expect("invalid OP base spec id");
    const OPCODE_CONFIG: &'static OpcodeConfig<OpEvmTypes> =
        &OpcodeConfig::<OpEvmTypes>::base::<Self>();
}

/// Selects the OP opcode table for the inherited Ethereum specification of an [`OpSpecId`].
#[derive(Clone, Copy, Debug)]
pub struct OpConfigSelector;

impl EvmConfigSelector<OpEvmTypes> for OpConfigSelector {
    type Config<const BASE_SPEC_ID: u32, const CUSTOM_SPEC_ID: u32> = OpConfig<BASE_SPEC_ID>;

    fn execution_config(spec_id: OpSpecId) -> ExecutionConfig<OpEvmTypes> {
        let spec_id = SpecId::from(spec_id);
        evm2::spec_to_generic!(spec_id, |SPEC_ID| ExecutionConfig::for_config::<OpConfig<SPEC_ID>>(
        ))
    }
}

/// OP block environment extension.
///
/// OP has no extra opcode-visible block fields. L1 block info is cached per block in
/// [`OpEvmExt`] instead, mirroring op-revm's chain context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpBlockEnvExt;

/// OP state owned by an EVM instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpEvmExt {
    /// Cached L1 block info, including the per-transaction L1 cost cache
    /// ([`L1BlockInfo::tx_l1_cost`]).
    ///
    /// Reloaded from the `L1Block` predeploy before a non-deposit transaction when
    /// [`L1BlockInfo::l2_block`] differs from the current block number. Set it with
    /// `l2_block = Some(block.number)` to inject L1 block info without a reload.
    pub l1_block_info: L1BlockInfo,
}

impl OpEvmExt {
    /// Creates the extension with injected L1 block info.
    pub const fn new(l1_block_info: L1BlockInfo) -> Self {
        Self { l1_block_info }
    }

    /// Clears the cached L1 cost of the current transaction.
    pub const fn clear_tx_l1_cost(&mut self) {
        self.l1_block_info.clear_tx_l1_cost();
    }
}

/// OP-specific transaction result fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpTxResultExt {
    /// Set when a deposit failed: it halted after Regolith, or failed validation. Only the caller
    /// nonce bump and the mint were kept. Corresponds to op-revm's `OpHaltReason::FailedDeposit`.
    pub failed_deposit: bool,
    /// Validation error that caused [`Self::failed_deposit`], if the deposit did not execute.
    pub deposit_error: Option<HandlerError>,
    /// L1 data fee credited to the `L1FeeVault`.
    pub l1_fee: U256,
    /// Operator fee credited to the `OperatorFeeVault` (on gas used, after the refund).
    pub operator_fee: U256,
}

/// Returns the evm2 [`Version`] for `spec` and `chain_id`.
///
/// OP uses mainnet gas parameters and features of the inherited Ethereum specification, like
/// alloy-op-evm's `CfgEnv::with_spec_and_mainnet_gas_params`.
pub fn op_version(spec: OpSpecId, chain_id: u64) -> Version {
    let mut version = Version::new(spec.into());
    version.chain_id = chain_id;
    version
}

/// Returns the execution config for `spec` with runtime `version` data.
pub fn op_execution_config(spec: OpSpecId, version: Version) -> ExecutionConfig<OpEvmTypes> {
    ExecutionConfig::for_spec_and_version(spec, version)
}

/// OP EVM factory.
///
/// Inherent methods mirror reth's evm2 `EvmFactory` trait without depending on reth.
#[derive(Clone, Debug, Default)]
pub struct OpEvmFactory;

impl OpEvmFactory {
    /// Returns the evm2 [`Version`] for `spec` and `chain_id`.
    pub fn version(&self, spec: OpSpecId, chain_id: u64) -> Version {
        op_version(spec, chain_id)
    }

    /// Returns the execution config for `spec` with runtime `version` data.
    pub fn execution_config(
        &self,
        spec: OpSpecId,
        version: Version,
    ) -> ExecutionConfig<OpEvmTypes> {
        op_execution_config(spec, version)
    }

    /// Returns the transaction registry for `spec`.
    pub fn tx_registry(&self, spec: OpSpecId) -> TxRegistry<OpEvmTypes, TxResult<OpEvmTypes>> {
        op_tx_registry(spec)
    }

    /// Returns the precompiles for `spec`.
    pub fn precompiles(&self, spec: OpSpecId) -> OpPrecompiles {
        OpPrecompiles::new(spec)
    }

    /// Installs the OP precompiles for the EVM's active spec.
    pub fn configure_evm(&self, evm: &mut OpEvm<'_>) {
        let spec = evm.config_spec_id();
        evm.set_precompiles(self.precompiles(spec));
    }

    /// Creates a configured OP EVM.
    pub fn create_evm<'a>(
        &self,
        spec: OpSpecId,
        version: Version,
        block: BlockEnv<OpEvmTypes>,
        db: impl DynDatabase + 'a,
    ) -> OpEvm<'a> {
        Evm::new_with_execution_config_and_ext(
            self.execution_config(spec, version),
            spec,
            block,
            self.tx_registry(spec),
            db,
            self.precompiles(spec),
            OpEvmExt::default(),
        )
    }
}
