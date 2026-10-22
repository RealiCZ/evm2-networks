//! Beryl B-20 factory, B-20 token, and policy registry precompiles.
//!
//! Each entry point mirrors the reference `base_common_precompiles` precompile wrapper
//! (`B20Factory`, `B20AssetPrecompile`, `B20StablecoinPrecompile`, `PolicyRegistryPrecompile`) and
//! runs its public storage dispatcher through the evm2-backed [`BaseStorageProvider`], so the token
//! and policy business logic is Base's own rather than a port.
//!
//! [`BaseStorageProvider`]: crate::storage::BaseStorageProvider

use alloy_primitives::{Address, Bytes};
use base_common_genesis::BaseUpgrade;
use base_common_precompiles::{
    B20AssetStorage, B20AssetToken, B20FactoryStorage, B20StablecoinStorage, B20StablecoinToken,
    B20Variant, PolicyRegistryStorage, PolicyVersions, UpgradeGatedStorageFeatures,
};
use base_precompile_storage::BasePrecompileError;
use evm2::{
    Evm,
    interpreter::{GasTracker, Message},
    precompiles::PrecompileResult,
};

use crate::{BaseEvmTypes, storage::run_native};

/// Precompile id of the B-20 factory, as installed by the reference.
pub(crate) const B20_FACTORY_ID: &str = "B20Factory";
/// Precompile id of the policy registry, as installed by the reference.
pub(crate) const POLICY_REGISTRY_ID: &str = "PolicyRegistryPrecompile";

/// Returns the B-20 token variant whose precompile `address` resolves to, matching the reference
/// `BerylLookup`.
pub(crate) fn b20_token_variant(address: &Address) -> Option<B20Variant> {
    B20Variant::from_address(*address)
}

/// Executes the B-20 factory precompile.
pub(crate) fn run_b20_factory(
    evm: &mut Evm<'_, BaseEvmTypes>,
    message: &Message<BaseEvmTypes>,
    gas: &mut GasTracker,
    upgrade: BaseUpgrade,
) -> PrecompileResult {
    let features = UpgradeGatedStorageFeatures::from_upgrade(upgrade);
    run_native(evm, message, gas, features, |ctx, calldata| {
        B20FactoryStorage::new(ctx).dispatch(ctx, calldata, upgrade)
    })
}

/// Executes the policy registry precompile.
pub(crate) fn run_policy_registry(
    evm: &mut Evm<'_, BaseEvmTypes>,
    message: &Message<BaseEvmTypes>,
    gas: &mut GasTracker,
    upgrade: BaseUpgrade,
) -> PrecompileResult {
    let features = UpgradeGatedStorageFeatures::from_upgrade(upgrade);
    run_native(evm, message, gas, features, |ctx, calldata| {
        PolicyRegistryStorage::new(ctx).dispatch(ctx, calldata, upgrade)
    })
}

/// Executes the B-20 token precompile of `variant` at the message's code address.
pub(crate) fn run_b20_token(
    evm: &mut Evm<'_, BaseEvmTypes>,
    message: &Message<BaseEvmTypes>,
    gas: &mut GasTracker,
    upgrade: BaseUpgrade,
    variant: B20Variant,
) -> PrecompileResult {
    let token = message.code_address;
    let features = UpgradeGatedStorageFeatures::from_upgrade(upgrade);
    run_native(evm, message, gas, features, |ctx, calldata| {
        let Some(version) = PolicyVersions::from_base_upgrade(upgrade) else {
            return BasePrecompileError::Revert(Bytes::new()).into_precompile_result(0, 0);
        };
        let policy = PolicyRegistryStorage::new(ctx);
        match variant {
            B20Variant::Asset => B20AssetToken::with_storage_and_policy(
                B20AssetStorage::from_address(token, ctx),
                policy,
                version,
            )
            .dispatch(ctx, calldata, upgrade),
            B20Variant::Stablecoin => B20StablecoinToken::with_storage_and_policy(
                B20StablecoinStorage::from_address(token, ctx),
                policy,
                version,
            )
            .dispatch(ctx, calldata, upgrade),
        }
    })
}
