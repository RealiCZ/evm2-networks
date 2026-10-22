//! Differential parity tests for the Beryl B-20 factory, B-20 tokens, and policy registry.
//!
//! Runs call sequences through `base-evm2`, where Base's storage dispatchers execute against evm2
//! state through the native storage provider, and through the revm-based `base-common-evm`
//! reference. Asserts the engines agree on every call's status, gas used, output, and logs, and on
//! the final code and full non-zero storage of the factory, registries, tokens, and users.

mod common;

use alloy_consensus::{Signed, TxLegacy};
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256};
use alloy_sol_types::{SolCall, SolError, SolValue};
use base_common_evm::BaseTransaction;
use base_common_genesis::BaseUpgrade;
use base_common_precompiles::{
    ActivationFeature, B20PolicyType, B20TokenRole, B20Variant, IActivationRegistry, IB20,
    IB20Factory, IPolicyRegistryV1 as IPolicyRegistry,
};
use base_evm2::{
    BaseTxEnvelope,
    precompiles::{ACTIVATION_REGISTRY_ADDRESS, B20_FACTORY_ADDRESS, POLICY_REGISTRY_ADDRESS},
};
use base_precompile_storage::DelegateCallNotAllowed;
use common::{CHAIN_ID, Compare, Env, Outcome, Seed, TxOutcome, run_evm2_seq, run_revm_seq};
use evm2::ethereum::TxEnvelope;
use revm::context::TxEnv;

const ADMIN: Address = Address::repeat_byte(0xad);
const ALICE: Address = Address::repeat_byte(0xa1);
const BOB: Address = Address::repeat_byte(0xb0);
const CAROL: Address = Address::repeat_byte(0xc4);
const STATIC_PROXY: Address = Address::repeat_byte(0x51);
const DELEGATE_PROXY: Address = Address::repeat_byte(0x52);
const CALL_PROXY: Address = Address::repeat_byte(0x53);
const CALLCODE_PROXY: Address = Address::repeat_byte(0x54);
const SALT: B256 = B256::repeat_byte(0x5a);
const UPGRADES: [BaseUpgrade; 3] = [BaseUpgrade::Beryl, BaseUpgrade::Cobalt, BaseUpgrade::Zenith];

fn asset_token() -> Address {
    B20Variant::Asset.compute_address(ALICE, SALT).0
}

fn stablecoin_token() -> Address {
    B20Variant::Stablecoin.compute_address(ALICE, SALT).0
}

/// A proxy forwarding its calldata to the address in the first calldata word's low 20 bytes with
/// `opcode` (`STATICCALL`, `DELEGATECALL`, `CALLCODE`, or `CALL` forwarding `CALLVALUE`) and
/// bubbling up the result. The forwarded calldata is everything after that word.
fn proxy(opcode: u8) -> Vec<u8> {
    // PUSH1 32 CALLDATASIZE SUB DUP1 PUSH1 32 PUSH0 CALLDATACOPY  (copy calldata[32..] to mem[0])
    let mut code = vec![0x60, 0x20, 0x36, 0x03, 0x80, 0x60, 0x20, 0x5f, 0x37];
    // stack: [len]. Build call args: retLen retOff argsLen argsOff [value] addr gas.
    // PUSH0 PUSH0 DUP3 PUSH0
    code.extend([0x5f, 0x5f, 0x82, 0x5f]);
    if matches!(opcode, 0xf1 | 0xf2) {
        // CALLVALUE
        code.push(0x34);
    }
    // PUSH0 CALLDATALOAD GAS <opcode>
    code.extend([0x5f, 0x35, 0x5a, opcode]);
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

fn raw(from: Address, to: Address, input: Vec<u8>) -> Call {
    Call { from, to, input, value: U256::ZERO, gas_limit: 3_000_000 }
}

fn call(from: Address, to: Address, input: impl SolCall) -> Call {
    raw(from, to, input.abi_encode())
}

/// Routes `call` through `proxy` towards its original target.
fn via(proxy: Address, call: Call) -> Call {
    let mut input = call.to.into_word().to_vec();
    input.extend(call.input);
    Call { to: proxy, input, ..call }
}

fn activate(feature: ActivationFeature) -> Call {
    call(
        ADMIN,
        ACTIVATION_REGISTRY_ADDRESS,
        IActivationRegistry::activateCall { feature: feature.id() },
    )
}

fn activate_all() -> Vec<Call> {
    vec![
        activate(ActivationFeature::B20Asset),
        activate(ActivationFeature::B20Stablecoin),
        activate(ActivationFeature::PolicyRegistry),
    ]
}

fn asset_params(name: &str) -> Bytes {
    IB20Factory::B20AssetCreateParams {
        version: 1,
        name: name.into(),
        symbol: "AST".into(),
        initialAdmin: ALICE,
        decimals: 18,
    }
    .abi_encode()
    .into()
}

fn stablecoin_params(currency: &str) -> Bytes {
    IB20Factory::B20StablecoinCreateParams {
        version: 1,
        name: "Stable".into(),
        symbol: "STB".into(),
        initialAdmin: ALICE,
        currency: currency.into(),
    }
    .abi_encode()
    .into()
}

fn create(variant: IB20Factory::B20Variant, params: Bytes, init_calls: Vec<Bytes>) -> Call {
    call(
        ALICE,
        B20_FACTORY_ADDRESS,
        IB20Factory::createB20Call { variant, salt: SALT, params, initCalls: init_calls },
    )
}

fn create_asset() -> Call {
    create(
        IB20Factory::B20Variant::ASSET,
        asset_params("Asset"),
        vec![
            IB20::grantRoleCall { role: B20TokenRole::Mint.id(), account: ALICE }
                .abi_encode()
                .into(),
            IB20::mintCall { to: ALICE, amount: U256::from(1_000_000) }.abi_encode().into(),
        ],
    )
}

fn create_stablecoin() -> Call {
    create(IB20Factory::B20Variant::STABLECOIN, stablecoin_params("USD"), vec![])
}

fn seeds() -> Vec<Seed> {
    let ether = U256::from(10u128.pow(18));
    vec![
        Seed::eoa(ADMIN, ether),
        Seed::eoa(ALICE, ether),
        Seed::eoa(BOB, ether),
        Seed::eoa(CAROL, ether),
        Seed::contract(STATIC_PROXY, proxy(0xfa)),
        Seed::contract(DELEGATE_PROXY, proxy(0xf4)),
        Seed::contract(CALL_PROXY, proxy(0xf1)),
        Seed::contract(CALLCODE_PROXY, proxy(0xf2)),
    ]
}

fn compare() -> Compare {
    [
        B20_FACTORY_ADDRESS,
        POLICY_REGISTRY_ADDRESS,
        ACTIVATION_REGISTRY_ADDRESS,
        asset_token(),
        stablecoin_token(),
        ADMIN,
        ALICE,
        BOB,
        CAROL,
        STATIC_PROXY,
        DELEGATE_PROXY,
        CALL_PROXY,
        CALLCODE_PROXY,
    ]
    .into_iter()
    .map(|address| (address, vec![]))
    .collect()
}

/// Runs `calls` in order on both engines and asserts parity, returning the per-call outcomes.
fn assert_parity(upgrade: BaseUpgrade, calls: &[Call]) -> Vec<TxOutcome> {
    let seeds = seeds();
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
    assert_eq!(evm2, revm, "B-20 outcomes diverged at {upgrade:?}");
    let Outcome::Executed { txs, .. } = evm2 else { panic!("a transaction was rejected") };
    txs
}

fn statuses(txs: &[TxOutcome]) -> Vec<bool> {
    txs.iter().map(|tx| tx.success).collect()
}

#[test]
fn create_then_token_getters_and_mutation() {
    let asset = asset_token();
    let stable = stablecoin_token();
    let mut calls = activate_all();
    calls.extend([
        call(
            BOB,
            B20_FACTORY_ADDRESS,
            IB20Factory::getB20AddressCall {
                variant: IB20Factory::B20Variant::ASSET,
                sender: ALICE,
                salt: SALT,
            },
        ),
        call(BOB, B20_FACTORY_ADDRESS, IB20Factory::isB20InitializedCall { token: asset }),
        create_asset(),
        create_stablecoin(),
        call(BOB, B20_FACTORY_ADDRESS, IB20Factory::isB20InitializedCall { token: asset }),
        call(BOB, B20_FACTORY_ADDRESS, IB20Factory::isB20Call { token: stable }),
        call(BOB, asset, IB20::nameCall {}),
        call(BOB, asset, IB20::symbolCall {}),
        call(BOB, asset, IB20::decimalsCall {}),
        call(BOB, asset, IB20::totalSupplyCall {}),
        call(BOB, asset, IB20::balanceOfCall { account: ALICE }),
        call(BOB, stable, IB20::nameCall {}),
        call(BOB, stable, IB20::decimalsCall {}),
        call(ALICE, asset, IB20::transferCall { to: BOB, amount: U256::from(400) }),
        call(ALICE, asset, IB20::approveCall { spender: CAROL, amount: U256::from(50) }),
        call(
            CAROL,
            asset,
            IB20::transferFromCall { from: ALICE, to: CAROL, amount: U256::from(30) },
        ),
        call(
            CAROL,
            asset,
            IB20::transferFromCall { from: ALICE, to: CAROL, amount: U256::from(30) },
        ),
        call(BOB, asset, IB20::transferCall { to: ALICE, amount: U256::from(401) }),
        call(BOB, asset, IB20::mintCall { to: BOB, amount: U256::from(1) }),
        call(ALICE, stable, IB20::grantRoleCall { role: B20TokenRole::Mint.id(), account: BOB }),
        call(BOB, stable, IB20::mintCall { to: CAROL, amount: U256::from(77) }),
        call(BOB, stable, IB20::balanceOfCall { account: CAROL }),
        call(ALICE, asset, IB20::updateNameCall { newName: "Renamed".into() }),
        call(BOB, asset, IB20::nameCall {}),
    ]);
    for upgrade in UPGRADES {
        let txs = assert_parity(upgrade, &calls);
        assert_eq!(
            statuses(&txs),
            [
                // Activation, factory views, and creation.
                true, true, true, true, true, true, true, true, true, // Token getters.
                true, true, true, true, true, true, true,
                // Transfer, approve, and the first transfer from succeed; the second exceeds the
                // allowance, Bob overspends, and Bob has no mint role.
                true, true, true, false, false, false,
                // Granting the stablecoin mint role to Bob lets him mint.
                true, true, true, // Renaming needs the metadata role.
                false, true,
            ],
            "statuses at {upgrade:?}"
        );
        assert_eq!(txs[3].output, asset.abi_encode(), "getB20Address at {upgrade:?}");
        assert_eq!(txs[5].output, asset.abi_encode(), "createB20 at {upgrade:?}");
        assert!(!txs[5].logs.is_empty(), "createB20 emits logs at {upgrade:?}");
    }
}

#[test]
fn policy_registry_gates_transfers() {
    let asset = asset_token();
    let sender_scope = B20PolicyType::TransferSender.id();
    let mut calls = activate_all();
    calls.extend([
        create_asset(),
        call(
            ALICE,
            POLICY_REGISTRY_ADDRESS,
            IPolicyRegistry::createPolicyCall {
                admin: ALICE,
                policyType: IPolicyRegistry::PolicyType::BLOCKLIST,
            },
        ),
        call(BOB, POLICY_REGISTRY_ADDRESS, IPolicyRegistry::policyExistsCall { policyId: 2 }),
        call(BOB, POLICY_REGISTRY_ADDRESS, IPolicyRegistry::policyAdminCall { policyId: 2 }),
        call(ALICE, asset, IB20::transferCall { to: BOB, amount: U256::from(10) }),
        call(
            ALICE,
            POLICY_REGISTRY_ADDRESS,
            IPolicyRegistry::updateBlocklistCall {
                policyId: 2,
                blocked: true,
                accounts: vec![BOB],
            },
        ),
        call(
            BOB,
            POLICY_REGISTRY_ADDRESS,
            IPolicyRegistry::updateBlocklistCall {
                policyId: 2,
                blocked: true,
                accounts: vec![ALICE],
            },
        ),
        call(
            BOB,
            POLICY_REGISTRY_ADDRESS,
            IPolicyRegistry::isAuthorizedCall { policyId: 2, account: BOB },
        ),
        call(ALICE, asset, IB20::updatePolicyCall { policyScope: sender_scope, newPolicyId: 2 }),
        call(BOB, asset, IB20::policyIdCall { policyScope: sender_scope }),
        call(BOB, asset, IB20::transferCall { to: ALICE, amount: U256::from(1) }),
        call(ALICE, asset, IB20::transferCall { to: CAROL, amount: U256::from(1) }),
        call(ALICE, asset, IB20::updatePolicyCall { policyScope: sender_scope, newPolicyId: 99 }),
    ]);
    for upgrade in UPGRADES {
        assert_parity(upgrade, &calls);
    }
}

#[test]
fn rejection_paths() {
    let asset = asset_token();
    let uncreated = B20Variant::Asset.compute_address(BOB, SALT).0;
    let mut with_value = create_stablecoin();
    with_value.value = U256::from(5);
    let mut token_with_value = call(BOB, asset, IB20::nameCall {});
    token_with_value.value = U256::from(5);
    let mut calls = vec![
        // Creation before the variant is activated.
        create_asset(),
    ];
    calls.extend(activate_all());
    calls.extend([
        create_asset(),
        // The deterministic address is taken.
        create_asset(),
        with_value,
        token_with_value,
        // Unsupported version, invalid currency, and unknown or malformed calldata.
        create(
            IB20Factory::B20Variant::ASSET,
            IB20Factory::B20AssetCreateParams {
                version: 2,
                name: "x".into(),
                symbol: "x".into(),
                initialAdmin: ALICE,
                decimals: 6,
            }
            .abi_encode()
            .into(),
            vec![],
        ),
        create(IB20Factory::B20Variant::STABLECOIN, stablecoin_params("XXX"), vec![]),
        create(
            IB20Factory::B20Variant::STABLECOIN,
            stablecoin_params("EUR"),
            vec![Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef])],
        ),
        raw(BOB, B20_FACTORY_ADDRESS, vec![]),
        raw(BOB, B20_FACTORY_ADDRESS, vec![0xde, 0xad, 0xbe, 0xef]),
        raw(BOB, B20_FACTORY_ADDRESS, IB20Factory::createB20Call::SELECTOR.to_vec()),
        raw(BOB, asset, vec![0x12]),
        raw(BOB, POLICY_REGISTRY_ADDRESS, vec![0xde, 0xad, 0xbe, 0xef]),
        // A prefixed address without a token, and a reserved variant byte (a plain account).
        call(BOB, uncreated, IB20::nameCall {}),
        call(BOB, uncreated, IB20::transferCall { to: ALICE, amount: U256::from(1) }),
        raw(
            BOB,
            {
                let mut bytes = uncreated.0.0;
                bytes[10] = 0x07;
                Address::from(bytes)
            },
            IB20::nameCall {}.abi_encode(),
        ),
        // Static, delegate, and callcode contexts.
        via(STATIC_PROXY, call(ALICE, asset, IB20::balanceOfCall { account: ALICE })),
        via(
            STATIC_PROXY,
            call(ALICE, asset, IB20::approveCall { spender: BOB, amount: U256::ONE }),
        ),
        via(STATIC_PROXY, create_stablecoin()),
        via(
            STATIC_PROXY,
            call(
                ALICE,
                POLICY_REGISTRY_ADDRESS,
                IPolicyRegistry::createPolicyCall {
                    admin: ALICE,
                    policyType: IPolicyRegistry::PolicyType::ALLOWLIST,
                },
            ),
        ),
        via(DELEGATE_PROXY, call(ALICE, asset, IB20::balanceOfCall { account: ALICE })),
        via(
            DELEGATE_PROXY,
            call(ALICE, B20_FACTORY_ADDRESS, IB20Factory::isB20Call { token: asset }),
        ),
        via(
            CALLCODE_PROXY,
            call(ALICE, POLICY_REGISTRY_ADDRESS, IPolicyRegistry::policyExistsCall { policyId: 1 }),
        ),
        via(CALL_PROXY, call(ALICE, asset, IB20::balanceOfCall { account: ALICE })),
        via(CALL_PROXY, create_stablecoin()),
    ]);
    let delegate_rejected: Bytes = DelegateCallNotAllowed {}.abi_encode().into();
    let exists: Bytes = IB20Factory::TokenAlreadyExists { token: asset }.abi_encode().into();
    for upgrade in UPGRADES {
        let txs = assert_parity(upgrade, &calls);
        assert!(!txs[0].success && txs[4].success, "activation gate at {upgrade:?}");
        assert_eq!((txs[5].success, &txs[5].output), (false, &exists), "{upgrade:?}");
        // A reserved variant byte is a plain account.
        assert!(txs[18].success && txs[18].output.is_empty(), "{upgrade:?}");
        // Static calls may read but not write; writes revert with empty data.
        assert!(txs[19].success, "{upgrade:?}");
        assert!(!txs[20].success && txs[20].output.is_empty(), "{upgrade:?}");
        for tx in &txs[23..26] {
            assert_eq!((tx.success, &tx.output), (false, &delegate_rejected), "{upgrade:?}");
        }
        assert!(txs[26].success, "{upgrade:?}");
    }
}

#[test]
fn out_of_gas() {
    let mut calls = activate_all();
    let create_limits = [40_000, 60_000, 100_000, 150_000, 200_000, 260_000, 275_000, 300_000];
    for gas_limit in create_limits {
        calls.push(Call { gas_limit, ..create_asset() });
    }
    let transfer_limits = [23_500, 25_000, 27_500, 30_000, 35_000, 40_000, 60_000];
    for gas_limit in transfer_limits {
        calls.push(Call {
            gas_limit,
            ..call(ALICE, asset_token(), IB20::transferCall { to: BOB, amount: U256::from(3) })
        });
    }
    for upgrade in UPGRADES {
        let txs = assert_parity(upgrade, &calls);
        let creates = &txs[3..3 + create_limits.len()];
        let transfers = &txs[3 + create_limits.len()..];
        // Both sweeps cross the out-of-gas boundary.
        for sweep in [creates, transfers] {
            assert!(!sweep[0].success, "first attempt runs out of gas at {upgrade:?}");
            assert!(sweep.last().unwrap().success, "last attempt succeeds at {upgrade:?}");
        }
    }
}

#[test]
fn absent_before_beryl() {
    let calls = [
        create_asset(),
        call(BOB, asset_token(), IB20::nameCall {}),
        call(BOB, POLICY_REGISTRY_ADDRESS, IPolicyRegistry::policyExistsCall { policyId: 1 }),
    ];
    let txs = assert_parity(BaseUpgrade::Azul, &calls);
    // Plain calls to empty accounts.
    assert!(txs.iter().all(|tx| tx.success && tx.output.is_empty() && tx.logs.is_empty()));
}
