//! Monad EVM factory.

use crate::{
    MonadBlockEnv, MonadEvm, MonadEvmExt, MonadEvmTypes, MonadHardfork, MonadPrecompiles,
    monad_execution_config, monad_tx_registry, monad_version,
};
use evm2::{Evm, ExecutionConfig, TxResult, Version, evm::DynDatabase, registry::TxRegistry};

/// Factory for Monad evm2 instances.
///
/// The inherent methods follow the naming of reth's evm2 `EvmFactory` trait.
#[derive(Clone, Debug, Default)]
pub struct MonadEvmFactory;

impl MonadEvmFactory {
    /// Returns the runtime version for a Monad hardfork and chain ID.
    pub const fn version(&self, spec: MonadHardfork, chain_id: u64) -> Version {
        monad_version(spec, chain_id)
    }

    /// Returns the execution config for a Monad hardfork and runtime version.
    pub fn execution_config(
        &self,
        spec: MonadHardfork,
        version: Version,
    ) -> ExecutionConfig<MonadEvmTypes> {
        monad_execution_config(spec, version)
    }

    /// Returns the transaction registry for a Monad hardfork.
    pub fn tx_registry(
        &self,
        spec: MonadHardfork,
    ) -> TxRegistry<MonadEvmTypes, TxResult<MonadEvmTypes>> {
        monad_tx_registry(spec)
    }

    /// Returns the precompile provider for a Monad hardfork.
    pub fn precompiles(&self, spec: MonadHardfork) -> MonadPrecompiles {
        MonadPrecompiles::new(spec)
    }

    /// Installs the Monad precompiles for the EVM's active hardfork.
    ///
    /// Call this after the EVM's execution config changes hardfork.
    pub fn configure_evm(&self, evm: &mut MonadEvm<'_>) {
        let spec = evm.config_spec_id();
        evm.set_precompiles(self.precompiles(spec));
    }

    /// Creates a Monad EVM for a hardfork, runtime version, block environment and database.
    pub fn create_evm<'a>(
        &self,
        spec: MonadHardfork,
        version: Version,
        block: MonadBlockEnv,
        db: impl DynDatabase + 'a,
    ) -> MonadEvm<'a> {
        let mut evm = Evm::new_with_execution_config_and_ext(
            self.execution_config(spec, version),
            spec,
            block,
            self.tx_registry(spec),
            db,
            self.precompiles(spec),
            MonadEvmExt::default(),
        );
        self.configure_evm(&mut evm);
        evm
    }
}
