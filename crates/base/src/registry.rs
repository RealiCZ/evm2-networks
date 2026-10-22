//! Base transaction registry.

use alloy_consensus::{TxEip1559, TxEip2930, TxLegacy};
use base_common_consensus::{DEPOSIT_TX_TYPE_ID, EIP8130_TX_TYPE_ID, TxDeposit};
use base_common_genesis::BaseUpgrade;
use evm2::{
    SpecId, TxResult,
    ethereum::{LazyTxEip7702, TxEnvelope, eip1559, eip2930, eip7702, legacy},
    registry::{TxRegistry, handler},
};

use crate::{
    BaseEvmTypes, BaseSpecId, BaseTxEnvelope, BaseTxHandlerHooks,
    deposit::{execute_deposit, prepare_deposit},
    eip8130::{execute_eip8130, prepare_eip8130},
};

fn as_legacy(tx: &BaseTxEnvelope) -> Option<&TxLegacy> {
    tx.as_standard().and_then(TxEnvelope::as_legacy)
}

fn as_eip2930(tx: &BaseTxEnvelope) -> Option<&TxEip2930> {
    tx.as_standard().and_then(TxEnvelope::as_eip2930)
}

fn as_eip1559(tx: &BaseTxEnvelope) -> Option<&TxEip1559> {
    tx.as_standard().and_then(TxEnvelope::as_eip1559)
}

fn as_eip7702(tx: &BaseTxEnvelope) -> Option<&LazyTxEip7702> {
    tx.as_standard().and_then(TxEnvelope::as_eip7702)
}

/// Returns the Base transaction registry for `spec`.
///
/// Registers the deposit handler (type `0x7e`) and the standard Ethereum handlers wired with
/// [`BaseTxHandlerHooks`]: legacy, EIP-2930, EIP-1559, and, from Isthmus (Prague), EIP-7702.
/// Zenith adds native EIP-8130 (type `0x79`) execution. EIP-4844 blob transactions are not
/// supported on Base.
pub fn base_tx_registry(spec: BaseSpecId) -> TxRegistry<BaseEvmTypes, TxResult<BaseEvmTypes>> {
    type Hooks = BaseTxHandlerHooks;

    let mut registry = TxRegistry::new()
        .with_handler(
            DEPOSIT_TX_TYPE_ID,
            BaseTxEnvelope::as_deposit,
            handler::<_, TxDeposit, _, _, _, _>(prepare_deposit, execute_deposit),
        )
        .with_handler(
            0,
            as_legacy,
            handler(
                legacy::prepare_with_hooks::<BaseEvmTypes, Hooks>,
                legacy::execute_prepared::<BaseEvmTypes, Hooks>,
            ),
        )
        .with_handler(
            1,
            as_eip2930,
            handler(
                eip2930::prepare_with_hooks::<BaseEvmTypes, Hooks>,
                eip2930::execute_prepared::<BaseEvmTypes, Hooks>,
            ),
        )
        .with_handler(
            2,
            as_eip1559,
            handler(
                eip1559::prepare_with_hooks::<BaseEvmTypes, Hooks>,
                eip1559::execute_prepared::<BaseEvmTypes, Hooks>,
            ),
        );
    if SpecId::from(spec).enables(SpecId::PRAGUE) {
        registry.register(
            4,
            as_eip7702,
            handler(
                eip7702::prepare_with_hooks::<BaseEvmTypes, Hooks>,
                eip7702::execute_prepared::<BaseEvmTypes, Hooks>,
            ),
        );
    }
    if spec.is_enabled_in(BaseUpgrade::Zenith) {
        registry.register(
            EIP8130_TX_TYPE_ID,
            BaseTxEnvelope::as_eip8130,
            handler(prepare_eip8130, execute_eip8130),
        );
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseEvmFactory, Eip8130Signed};
    use alloy_primitives::{Address, Bytes};
    use base_common_consensus::{EIP8130_TX_TYPE_ID, TxEip8130};
    use evm2::{env::BlockEnv, evm::InMemoryDB, registry::HandlerError};

    #[test]
    fn registers_supported_transaction_types() {
        for (upgrade, eip7702) in [
            (BaseUpgrade::Regolith, false),
            (BaseUpgrade::Jovian, true),
            (BaseUpgrade::Beryl, true),
        ] {
            let registry = base_tx_registry(BaseSpecId::new(upgrade));
            for ty in [DEPOSIT_TX_TYPE_ID, 0, 1, 2] {
                assert!(registry.contains(ty), "type {ty} at {upgrade:?}");
            }
            assert_eq!(registry.contains(4), eip7702, "EIP-7702 at {upgrade:?}");
            assert!(!registry.contains(3), "EIP-4844 is not supported on Base");
            assert!(!registry.contains(EIP8130_TX_TYPE_ID));
        }
        assert!(
            base_tx_registry(BaseSpecId::new(BaseUpgrade::Zenith)).contains(EIP8130_TX_TYPE_ID)
        );
    }

    #[test]
    fn eip8130_is_rejected_before_zenith() {
        let spec = BaseSpecId::new(BaseUpgrade::Denim);
        let factory = BaseEvmFactory::default();
        let mut evm = factory.create_evm(
            spec,
            factory.version(spec, 8453),
            BlockEnv::<BaseEvmTypes>::default(),
            InMemoryDB::default(),
        );
        let tx = BaseTxEnvelope::from(Eip8130Signed::new(
            TxEip8130::default(),
            Bytes::new(),
            Bytes::new(),
        ));
        let result = evm.transact(&tx.with_signer(Address::ZERO)).map(|tx| tx.discard());
        assert_eq!(result, Err(HandlerError::UnsupportedTransactionType(EIP8130_TX_TYPE_ID)));
    }
}
