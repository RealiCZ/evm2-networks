//! Base evm2 factory.

use alloy_primitives::Address;
use evm2::{
    ExecutionConfig, TxResult, Version, env::BlockEnv, evm::DynDatabase, registry::TxRegistry,
};

use crate::{
    BaseEvm, BaseEvmExt, BaseEvmTypes, BasePrecompiles, BaseSpecId, base_execution_config,
    base_tx_registry, base_version,
};

/// Factory for [`BaseEvm`] instances.
///
/// Carries the activation registry admin address, like `base_common_evm::BaseEvmFactory`. Its
/// inherent methods follow the naming of reth's evm2 `EvmFactory` trait.
#[derive(Clone, Debug, Default)]
pub struct BaseEvmFactory {
    activation_admin_address: Option<Address>,
}

impl BaseEvmFactory {
    /// Creates a factory with the given activation registry admin address.
    pub const fn new(activation_admin_address: Option<Address>) -> Self {
        Self { activation_admin_address }
    }

    /// Returns the activation registry admin address.
    pub const fn activation_admin_address(&self) -> Option<Address> {
        self.activation_admin_address
    }

    /// Returns this factory with the activation registry admin address set.
    #[must_use]
    pub const fn with_activation_admin_address(
        mut self,
        activation_admin_address: Option<Address>,
    ) -> Self {
        self.activation_admin_address = activation_admin_address;
        self
    }

    /// Sets the activation registry admin address.
    pub const fn set_activation_admin_address(
        &mut self,
        activation_admin_address: Option<Address>,
    ) {
        self.activation_admin_address = activation_admin_address;
    }

    /// Returns the evm2 version for `spec` on chain `chain_id`.
    pub fn version(&self, spec: BaseSpecId, chain_id: u64) -> Version {
        base_version(spec, chain_id)
    }

    /// Returns the execution config for `spec` with the runtime `version`.
    pub fn execution_config(
        &self,
        spec: BaseSpecId,
        version: Version,
    ) -> ExecutionConfig<BaseEvmTypes> {
        base_execution_config(spec, version)
    }

    /// Returns the transaction registry for `spec`.
    pub fn tx_registry(
        &self,
        spec: BaseSpecId,
    ) -> TxRegistry<BaseEvmTypes, TxResult<BaseEvmTypes>> {
        base_tx_registry(spec)
    }

    /// Returns the precompiles for `spec`, configured with this factory's activation admin.
    pub fn precompiles(&self, spec: BaseSpecId) -> BasePrecompiles {
        BasePrecompiles::new(spec).with_activation_admin_address(self.activation_admin_address)
    }

    /// Installs this factory's precompiles for the EVM's active spec.
    pub fn configure_evm(&self, evm: &mut BaseEvm<'_>) {
        let spec = evm.config_spec_id();
        evm.set_precompiles(self.precompiles(spec));
    }

    /// Creates a configured [`BaseEvm`] over `db`.
    pub fn create_evm<'a>(
        &self,
        spec: BaseSpecId,
        version: Version,
        block: BlockEnv<BaseEvmTypes>,
        db: impl DynDatabase + 'a,
    ) -> BaseEvm<'a> {
        BaseEvm::new_with_execution_config_and_ext(
            self.execution_config(spec, version),
            spec,
            block,
            self.tx_registry(spec),
            db,
            self.precompiles(spec),
            BaseEvmExt::default(),
        )
    }
}
