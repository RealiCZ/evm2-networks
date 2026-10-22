//! Cobalt transaction-context getters over the running transaction's origin.

use base_common_precompiles::{TxContextStorage, UpgradeGatedStorageFeatures};
use evm2::{
    Evm,
    interpreter::{GasTracker, Message},
    precompiles::PrecompileResult,
};

use crate::{BaseEvmTypes, storage::run_native};

pub(crate) fn run_tx_context(
    evm: &mut Evm<'_, BaseEvmTypes>,
    message: &Message<BaseEvmTypes>,
    gas: &mut GasTracker,
) -> PrecompileResult {
    let features = UpgradeGatedStorageFeatures::from_upgrade(evm.config_spec_id().upgrade());
    run_native(evm, message, gas, features, |ctx, calldata| {
        TxContextStorage::new(ctx).dispatch(ctx, calldata)
    })
}
