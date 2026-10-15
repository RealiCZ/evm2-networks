//! Differential parity tests for the Beryl activation registry precompile.
//!
//! Runs sequences of calls to the activation registry through `base-evm2` and the revm-based
//! `base-common-evm` reference, both configured with the same activation admin, and asserts the
//! engines agree on every call's status, gas used, output, and logs, and on the registry's final
//! code and storage.

mod common;

use alloy_consensus::{Signed, TxLegacy};
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256};
use alloy_sol_types::{SolCall, sol};
use base_common_evm::BaseTransaction;
use base_common_genesis::BaseUpgrade;
use base_evm2::{BaseTxEnvelope, precompiles::ACTIVATION_REGISTRY_ADDRESS};
use common::{CHAIN_ID, Compare, Env, Outcome, Seed, run_evm2_seq, run_revm_seq};
use evm2::ethereum::TxEnvelope;
use revm::context::TxEnv;

sol! {
    interface IActivationRegistry {
        function isActivated(bytes32 feature) external view returns (bool);
        function checkActivated(bytes32 feature) external view;
        function admin() external view returns (address);
        function setAdmin(address newAdmin) external;
        function activate(bytes32 feature) external;
        function deactivate(bytes32 feature) external;
    }
}

const ADMIN: Address = Address::repeat_byte(0xad);
const NEW_ADMIN: Address = Address::repeat_byte(0xae);
const OTHER: Address = Address::repeat_byte(0x0b);
const STATIC_PROXY: Address = Address::repeat_byte(0x51);
const DELEGATE_PROXY: Address = Address::repeat_byte(0x52);
const CALL_PROXY: Address = Address::repeat_byte(0x53);
/// `keccak256("base.b20_asset")`.
const FEATURE: B256 = B256::new([
    0xcd, 0xcc, 0x77, 0x2f, 0xe4, 0xcb, 0xdb, 0x10, 0x29, 0xf8, 0x22, 0x86, 0x11, 0x76, 0xd0, 0x9e,
    0x64, 0x6d, 0xb9, 0x67, 0x23, 0xd4, 0xc1, 0xe8, 0x2d, 0xdf, 0xde, 0xb8, 0x16, 0x3e, 0xf5, 0x4c,
]);
const NAMESPACE_ROOT: U256 = U256::from_be_bytes([
    0x43, 0xee, 0x1b, 0xbe, 0x25, 0xe9, 0x88, 0x52, 0x1c, 0xcc, 0xd8, 0xb2, 0xc8, 0xfb, 0xd3, 0x8c,
    0x82, 0x87, 0xeb, 0xff, 0x8e, 0x07, 0x4e, 0x82, 0x5a, 0x70, 0xdf, 0xd3, 0x88, 0x5c, 0xce, 0x00,
]);

/// A proxy forwarding its calldata to the registry with `opcode` (`STATICCALL`, `DELEGATECALL`,
/// or `CALL` with zero value) and bubbling up the result.
fn proxy(opcode: u8) -> Vec<u8> {
    // CALLDATASIZE PUSH0 PUSH0 CALLDATACOPY
    let mut code = vec![0x36, 0x5f, 0x5f, 0x37];
    // PUSH0 PUSH0 CALLDATASIZE PUSH0 [PUSH0 for CALL value]
    code.extend([0x5f, 0x5f, 0x36, 0x5f]);
    if opcode == 0xf1 {
        code.push(0x5f);
    }
    // PUSH20 registry GAS <opcode>
    code.push(0x73);
    code.extend_from_slice(ACTIVATION_REGISTRY_ADDRESS.as_slice());
    code.extend([0x5a, opcode]);
    // RETURNDATASIZE PUSH0 PUSH0 RETURNDATACOPY
    code.extend([0x3d, 0x5f, 0x5f, 0x3e]);
    // PUSH1 <ok> JUMPI RETURNDATASIZE PUSH0 REVERT JUMPDEST RETURNDATASIZE PUSH0 RETURN
    let ok = code.len() as u8 + 6;
    code.extend([0x60, ok, 0x57, 0x3d, 0x5f, 0xfd, 0x5b, 0x3d, 0x5f, 0xf3]);
    code
}

#[derive(Clone)]
struct Call {
    from: Address,
    to: Address,
    input: Vec<u8>,
    value: U256,
    gas_limit: u64,
}

fn call(from: Address, input: impl SolCall) -> Call {
    Call {
        from,
        to: ACTIVATION_REGISTRY_ADDRESS,
        input: input.abi_encode(),
        value: U256::ZERO,
        gas_limit: 200_000,
    }
}

fn raw(from: Address, input: Vec<u8>) -> Call {
    Call { from, to: ACTIVATION_REGISTRY_ADDRESS, input, value: U256::ZERO, gas_limit: 200_000 }
}

fn via(proxy: Address, call: Call) -> Call {
    Call { to: proxy, ..call }
}

fn seeds(registry_code: Option<Vec<u8>>) -> Vec<Seed> {
    let ether = U256::from(10u128.pow(18));
    let mut seeds = vec![
        Seed::eoa(ADMIN, ether),
        Seed::eoa(NEW_ADMIN, ether),
        Seed::eoa(OTHER, ether),
        Seed::contract(STATIC_PROXY, proxy(0xfa)),
        Seed::contract(DELEGATE_PROXY, proxy(0xf4)),
        Seed::contract(CALL_PROXY, proxy(0xf1)),
    ];
    if let Some(code) = registry_code {
        seeds.push(Seed::contract(ACTIVATION_REGISTRY_ADDRESS, code));
    }
    seeds
}

fn compare() -> Compare {
    let mut feature_slot = [0u8; 64];
    feature_slot[..32].copy_from_slice(FEATURE.as_slice());
    feature_slot[32..].copy_from_slice(&NAMESPACE_ROOT.to_be_bytes::<32>());
    let feature_slot = U256::from_be_bytes(keccak256(feature_slot).0);
    vec![
        (ACTIVATION_REGISTRY_ADDRESS, vec![feature_slot, NAMESPACE_ROOT + U256::ONE]),
        (ADMIN, vec![]),
        (NEW_ADMIN, vec![]),
        (OTHER, vec![]),
    ]
}

/// Runs `calls` in order on both engines and asserts parity, returning the evm2 outcome.
fn assert_parity(upgrade: BaseUpgrade, registry_code: Option<Vec<u8>>, calls: &[Call]) -> Outcome {
    let seeds = seeds(registry_code);
    let env = Env { activation_admin: Some(ADMIN), ..Env::new(upgrade) };
    let mut nonces = std::collections::HashMap::<Address, u64>::new();
    let mut evm2_txs = Vec::new();
    let mut revm_txs = Vec::new();
    for call in calls {
        let nonce = nonces.entry(call.from).or_default();
        let tx = TxLegacy {
            chain_id: Some(CHAIN_ID),
            nonce: *nonce,
            gas_price: 1,
            gas_limit: call.gas_limit,
            to: TxKind::Call(call.to),
            value: call.value,
            input: Bytes::from(call.input.clone()),
        };
        let encoded: Bytes =
            Signed::new_unhashed(tx.clone(), Signature::test_signature()).encoded_2718().into();
        evm2_txs
            .push((BaseTxEnvelope::standard(TxEnvelope::Legacy(tx), encoded.clone()), call.from));
        revm_txs.push(
            BaseTransaction::builder()
                .base(
                    TxEnv::builder()
                        .tx_type(Some(0))
                        .caller(call.from)
                        .nonce(*nonce)
                        .chain_id(Some(CHAIN_ID))
                        .gas_price(1)
                        .gas_limit(call.gas_limit)
                        .kind(TxKind::Call(call.to))
                        .value(call.value)
                        .data(Bytes::from(call.input.clone())),
                )
                .enveloped_tx(Some(encoded))
                .build()
                .unwrap(),
        );
        *nonce += 1;
    }
    let evm2 = run_evm2_seq(env, &seeds, evm2_txs, &compare());
    let revm = run_revm_seq(env, &seeds, revm_txs, &compare());
    assert_eq!(evm2, revm, "activation registry outcomes diverged at {upgrade:?}");
    evm2
}

fn statuses(outcome: &Outcome) -> Vec<bool> {
    let Outcome::Executed { txs, .. } = outcome else { panic!("a transaction was rejected") };
    txs.iter().map(|tx| tx.success).collect()
}

const UPGRADES: [BaseUpgrade; 2] = [BaseUpgrade::Beryl, BaseUpgrade::Cobalt];

#[test]
fn activation_lifecycle() {
    let feature = FEATURE;
    let calls = [
        call(OTHER, IActivationRegistry::adminCall {}),
        call(OTHER, IActivationRegistry::isActivatedCall { feature }),
        call(OTHER, IActivationRegistry::checkActivatedCall { feature }),
        call(OTHER, IActivationRegistry::activateCall { feature }),
        call(ADMIN, IActivationRegistry::activateCall { feature }),
        call(OTHER, IActivationRegistry::isActivatedCall { feature }),
        call(OTHER, IActivationRegistry::checkActivatedCall { feature }),
        call(ADMIN, IActivationRegistry::activateCall { feature }),
        call(ADMIN, IActivationRegistry::deactivateCall { feature }),
        call(ADMIN, IActivationRegistry::deactivateCall { feature }),
        call(ADMIN, IActivationRegistry::activateCall { feature }),
    ];
    for upgrade in UPGRADES {
        let outcome = assert_parity(upgrade, None, &calls);
        assert_eq!(
            statuses(&outcome),
            [true, true, false, false, true, true, true, false, true, false, true]
        );
        // Sentinel code planted by a foundry-style fork: only the code deposit is charged.
        assert_parity(upgrade, Some(vec![0xfe]), &calls);
    }
}

#[test]
fn admin_rotation() {
    let feature = FEATURE;
    let calls = [
        call(ADMIN, IActivationRegistry::setAdminCall { newAdmin: NEW_ADMIN }),
        call(OTHER, IActivationRegistry::adminCall {}),
        call(NEW_ADMIN, IActivationRegistry::activateCall { feature }),
        call(ADMIN, IActivationRegistry::activateCall { feature }),
        call(NEW_ADMIN, IActivationRegistry::setAdminCall { newAdmin: Address::ZERO }),
        call(OTHER, IActivationRegistry::setAdminCall { newAdmin: OTHER }),
    ];
    let beryl = assert_parity(BaseUpgrade::Beryl, None, &calls);
    // Before Cobalt `setAdmin` is an unknown selector and the static admin stays in charge.
    assert_eq!(statuses(&beryl), [false, true, false, true, false, false]);
    let cobalt = assert_parity(BaseUpgrade::Cobalt, None, &calls);
    assert_eq!(statuses(&cobalt), [true, true, true, false, false, false]);
}

#[test]
fn malformed_calldata_and_value() {
    let feature = FEATURE;
    let mut with_value = call(OTHER, IActivationRegistry::isActivatedCall { feature });
    with_value.value = U256::from(7);
    let calls = [
        raw(OTHER, vec![]),
        raw(OTHER, vec![0x12, 0x34]),
        raw(OTHER, vec![0xde, 0xad, 0xbe, 0xef]),
        raw(OTHER, IActivationRegistry::activateCall::SELECTOR.to_vec()),
        raw(OTHER, {
            // Dirty high bytes in the address argument fail validating decode.
            let mut input = IActivationRegistry::setAdminCall { newAdmin: OTHER }.abi_encode();
            input[4] = 0xff;
            input
        }),
        with_value,
    ];
    for upgrade in UPGRADES {
        assert_parity(upgrade, None, &calls);
    }
}

#[test]
fn proxied_calls() {
    let feature = FEATURE;
    let calls = [
        via(STATIC_PROXY, call(ADMIN, IActivationRegistry::activateCall { feature })),
        via(STATIC_PROXY, call(ADMIN, IActivationRegistry::isActivatedCall { feature })),
        via(DELEGATE_PROXY, call(ADMIN, IActivationRegistry::isActivatedCall { feature })),
        via(CALL_PROXY, call(ADMIN, IActivationRegistry::activateCall { feature })),
        via(CALL_PROXY, call(ADMIN, IActivationRegistry::adminCall {})),
    ];
    for upgrade in UPGRADES {
        let outcome = assert_parity(upgrade, None, &calls);
        assert_eq!(statuses(&outcome), [false, true, false, false, true]);
    }
}

#[test]
fn out_of_gas() {
    let feature = FEATURE;
    let base = call(ADMIN, IActivationRegistry::activateCall { feature });
    let calls: Vec<_> = [22_000, 24_000, 26_000, 30_000, 45_000, 50_000, 60_000, 70_000, 80_000]
        .into_iter()
        .map(|gas_limit| Call { gas_limit, ..base.clone() })
        .collect();
    for upgrade in UPGRADES {
        assert_parity(upgrade, None, &calls);
    }
}

#[test]
fn no_admin_configured() {
    let calls = [call(ADMIN, IActivationRegistry::activateCall { feature: FEATURE })];
    for upgrade in UPGRADES {
        let seeds = seeds(None);
        let outcome = {
            // Same as `assert_parity`, but with no activation admin on either engine.
            let env = Env::new(upgrade);
            let tx = TxLegacy {
                chain_id: Some(CHAIN_ID),
                gas_price: 1,
                gas_limit: calls[0].gas_limit,
                to: TxKind::Call(ACTIVATION_REGISTRY_ADDRESS),
                input: calls[0].input.clone().into(),
                ..Default::default()
            };
            let encoded: Bytes =
                Signed::new_unhashed(tx.clone(), Signature::test_signature()).encoded_2718().into();
            let revm_tx = BaseTransaction::builder()
                .base(
                    TxEnv::builder()
                        .tx_type(Some(0))
                        .caller(ADMIN)
                        .chain_id(Some(CHAIN_ID))
                        .gas_price(1)
                        .gas_limit(calls[0].gas_limit)
                        .kind(TxKind::Call(ACTIVATION_REGISTRY_ADDRESS))
                        .data(calls[0].input.clone().into()),
                )
                .enveloped_tx(Some(encoded.clone()))
                .build()
                .unwrap();
            let evm2 = run_evm2_seq(
                env,
                &seeds,
                vec![(BaseTxEnvelope::standard(TxEnvelope::Legacy(tx), encoded), ADMIN)],
                &compare(),
            );
            let revm = run_revm_seq(env, &seeds, vec![revm_tx], &compare());
            assert_eq!(evm2, revm);
            evm2
        };
        assert_eq!(statuses(&outcome), [false]);
    }
}

#[test]
fn registry_absent_before_beryl() {
    let calls = [call(OTHER, IActivationRegistry::adminCall {})];
    let outcome = assert_parity(BaseUpgrade::Azul, None, &calls);
    let Outcome::Executed { txs, .. } = outcome else { panic!("rejected") };
    // A plain call to an empty account.
    assert!(txs[0].success);
    assert!(txs[0].output.is_empty());
}
