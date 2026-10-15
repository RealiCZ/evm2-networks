//! Differential parity tests for standard (non-deposit) transactions.
//!
//! Runs the same transaction through `base-evm2` and the revm-based `base-common-evm` reference
//! and asserts both engines agree on status, gas used, output, logs, and the post-state of the
//! caller, the target, the coinbase, and the L1 fee, base fee, and operator fee vaults. The L1 fee
//! inputs are read from the `L1Block` predeploy storage on both sides, exercising the L1 block
//! info load.
//!
//! Adapted from the Base team's `base-common-evm2` `standard_fee_parity` harness.

mod common;

use alloy_consensus::{Signed, TxEip1559, TxLegacy};
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256};
use base_common_consensus::{Predeploys, SystemAddresses, TxDeposit};
use base_common_evm::BaseTransaction;
use base_common_genesis::BaseUpgrade;
use base_evm2::BaseTxEnvelope;
use common::{
    CHAIN_ID, COINBASE, Compare, Env, Outcome, Seed, run_evm2, run_evm2_seq, run_revm, run_revm_seq,
};
use evm2::ethereum::TxEnvelope;
use revm::context::TxEnv;

const SENDER: Address = Address::repeat_byte(0x11);
const TARGET: Address = Address::repeat_byte(0x22);
const BASEFEE: u64 = 500;
const ETHER: u128 = 1_000_000_000_000_000_000;

/// Upgrades covering each fee branch: Bedrock-style L1 cost (Regolith), Ecotone linear cost,
/// Fjord `FastLZ` cost, Isthmus operator fee, Jovian operator fee multiplier, and Azul (Osaka).
const UPGRADES: [BaseUpgrade; 7] = [
    BaseUpgrade::Regolith,
    BaseUpgrade::Ecotone,
    BaseUpgrade::Fjord,
    BaseUpgrade::Granite,
    BaseUpgrade::Isthmus,
    BaseUpgrade::Jovian,
    BaseUpgrade::Azul,
];

#[derive(Clone)]
enum Kind {
    Legacy { gas_price: u128 },
    Eip1559 { max_fee: u128, priority_fee: u128 },
}

#[derive(Clone)]
struct Fixture {
    seeds: Vec<Seed>,
    kind: Kind,
    to: TxKind,
    value: U256,
    gas_limit: u64,
    input: Vec<u8>,
    /// Whether to price the L1 fees over the real EIP-2718 encoding, or over empty bytes.
    enveloped: bool,
    compare: Compare,
}

impl Fixture {
    fn new(kind: Kind) -> Self {
        Self {
            seeds: vec![Seed::eoa(SENDER, U256::from(ETHER))],
            kind,
            to: TxKind::Call(TARGET),
            value: U256::from(12_345),
            gas_limit: 100_000,
            input: vec![],
            enveloped: true,
            compare: vec![
                (SENDER, vec![]),
                (TARGET, vec![U256::ZERO]),
                (COINBASE, vec![]),
                (Predeploys::L1_FEE_VAULT, vec![]),
                (Predeploys::BASE_FEE_VAULT, vec![]),
                (Predeploys::OPERATOR_FEE_VAULT, vec![]),
            ],
        }
    }

    fn eip1559() -> Self {
        Self::new(Kind::Eip1559 { max_fee: 1_000, priority_fee: 100 })
    }

    fn legacy() -> Self {
        Self::new(Kind::Legacy { gas_price: 800 })
    }

    fn with_contract(mut self, code: Vec<u8>, storage: Vec<(U256, U256)>) -> Self {
        self.seeds.push(Seed::contract(TARGET, code).with_storage(storage));
        self
    }

    fn signed(&self) -> (TxEnvelope, Bytes) {
        let input = Bytes::from(self.input.clone());
        match self.kind {
            Kind::Legacy { gas_price } => {
                let tx = TxLegacy {
                    chain_id: Some(CHAIN_ID),
                    nonce: 0,
                    gas_price,
                    gas_limit: self.gas_limit,
                    to: self.to,
                    value: self.value,
                    input,
                };
                let encoded =
                    Signed::new_unhashed(tx.clone(), Signature::test_signature()).encoded_2718();
                (TxEnvelope::Legacy(tx), encoded.into())
            }
            Kind::Eip1559 { max_fee, priority_fee } => {
                let tx = TxEip1559 {
                    chain_id: CHAIN_ID,
                    nonce: 0,
                    gas_limit: self.gas_limit,
                    max_fee_per_gas: max_fee,
                    max_priority_fee_per_gas: priority_fee,
                    to: self.to,
                    value: self.value,
                    input,
                    access_list: Default::default(),
                };
                let encoded =
                    Signed::new_unhashed(tx.clone(), Signature::test_signature()).encoded_2718();
                (TxEnvelope::Eip1559(tx), encoded.into())
            }
        }
    }

    fn enveloped_bytes(&self) -> Bytes {
        if self.enveloped { self.signed().1 } else { Bytes::new() }
    }

    fn evm2_tx(&self) -> BaseTxEnvelope {
        BaseTxEnvelope::standard(self.signed().0, self.enveloped_bytes())
    }

    fn revm_tx(&self) -> BaseTransaction<TxEnv> {
        let base = TxEnv::builder()
            .caller(SENDER)
            .nonce(0)
            .chain_id(Some(CHAIN_ID))
            .kind(self.to)
            .gas_limit(self.gas_limit)
            .value(self.value)
            .data(Bytes::from(self.input.clone()));
        let base = match self.kind {
            Kind::Legacy { gas_price } => base.tx_type(Some(0)).gas_price(gas_price),
            Kind::Eip1559 { max_fee, priority_fee } => {
                base.tx_type(Some(2)).max_fee_per_gas(max_fee).gas_priority_fee(Some(priority_fee))
            }
        };
        BaseTransaction::builder()
            .base(base)
            .enveloped_tx(Some(self.enveloped_bytes()))
            .build()
            .expect("valid revm transaction")
    }

    fn outcomes(&self, env: Env) -> (Outcome, Outcome) {
        let evm2 = run_evm2(env, &self.seeds, self.evm2_tx(), SENDER, &self.compare);
        let revm = run_revm(env, &self.seeds, self.revm_tx(), &self.compare);
        (evm2, revm)
    }

    fn assert_parity_with(&self, env: impl Fn(BaseUpgrade) -> Env) {
        for upgrade in UPGRADES {
            let (evm2, revm) = self.outcomes(env(upgrade));
            assert_eq!(evm2, revm, "standard transaction outcomes diverged at {upgrade:?}");
        }
    }

    fn assert_parity(&self) {
        self.assert_parity_with(|upgrade| Env::new(upgrade).with_basefee(BASEFEE));
    }
}

/// Asserts the fixture produced a non-trivial L1 fee somewhere, so parity is not vacuous.
fn assert_l1_fee_charged(fixture: &Fixture, upgrade: BaseUpgrade) {
    let (outcome, _) = fixture.outcomes(Env::new(upgrade).with_basefee(BASEFEE));
    let Outcome::Executed { accounts, .. } = outcome else { panic!("transaction rejected") };
    assert!(!accounts[&Predeploys::L1_FEE_VAULT].balance.is_zero(), "no L1 fee at {upgrade:?}");
    if upgrade >= BaseUpgrade::Isthmus {
        assert!(!accounts[&Predeploys::OPERATOR_FEE_VAULT].balance.is_zero());
    }
}

#[test]
fn eip1559_transfer() {
    let fixture = Fixture::eip1559();
    fixture.assert_parity();
    assert_l1_fee_charged(&fixture, BaseUpgrade::Fjord);
    assert_l1_fee_charged(&fixture, BaseUpgrade::Jovian);
}

#[test]
fn legacy_transfer() {
    Fixture::legacy().assert_parity();
}

#[test]
fn contract_call_with_refund_and_log() {
    // PUSH1 0x00 PUSH1 0x00 SSTORE PUSH1 0x00 PUSH1 0x00 LOG0 STOP: clears slot 0 for a refund.
    let code = vec![0x60, 0x00, 0x60, 0x00, 0x55, 0x60, 0x00, 0x60, 0x00, 0xa0, 0x00];
    Fixture::eip1559().with_contract(code, vec![(U256::ZERO, U256::from(1))]).assert_parity();
}

#[test]
fn reverted_call() {
    // PUSH1 0x2a PUSH1 0x00 SSTORE PUSH1 0x00 PUSH1 0x00 REVERT.
    let code = vec![0x60, 0x2a, 0x60, 0x00, 0x55, 0x60, 0x00, 0x60, 0x00, 0xfd];
    Fixture::legacy().with_contract(code, vec![]).assert_parity();
}

#[test]
fn halted_call() {
    Fixture::eip1559().with_contract(vec![0xfe], vec![]).assert_parity();
}

#[test]
fn create_transaction() {
    // PUSH1 0x00 PUSH1 0x00 RETURN.
    let mut fixture = Fixture::eip1559();
    fixture.to = TxKind::Create;
    fixture.input = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
    fixture.compare.push((SENDER.create(0), vec![]));
    fixture.assert_parity();
}

#[test]
fn calldata_floor() {
    let mut fixture = Fixture::eip1559();
    fixture.input = vec![0x01; 1_000];
    fixture.assert_parity();
}

#[test]
fn fee_charge_disabled_still_credits_vaults() {
    Fixture::eip1559().assert_parity_with(|upgrade| {
        let mut env = Env::new(upgrade).with_basefee(BASEFEE);
        env.fee_charge = false;
        env
    });
}

#[test]
fn foundry_style_simulation_without_payload() {
    // Fee charging, balance checks, and base fee checks off, a zero gas price, and no L1 payload:
    // the configuration Foundry uses in tests.
    let mut fixture = Fixture::legacy();
    fixture.kind = Kind::Legacy { gas_price: 0 };
    fixture.enveloped = false;
    fixture.seeds = vec![];
    fixture.value = U256::ZERO;
    fixture.assert_parity_with(|upgrade| {
        let mut env = Env::new(upgrade).with_basefee(BASEFEE);
        env.fee_charge = false;
        env.balance_check = false;
        env.base_fee_check = false;
        env
    });
}

#[test]
fn caller_short_of_l1_fee_is_rejected() {
    // Enough for the maximum gas cost and the value, but not the L1 data and operator fees.
    let mut fixture = Fixture::eip1559();
    let max_cost = U256::from(fixture.gas_limit) * U256::from(1_000) + fixture.value;
    fixture.seeds = vec![Seed::eoa(SENDER, max_cost)];
    for upgrade in UPGRADES {
        let (evm2, revm) = fixture.outcomes(Env::new(upgrade).with_basefee(BASEFEE));
        assert_eq!(revm, Outcome::Invalid, "reference accepted an underfunded caller");
        assert_eq!(evm2, Outcome::Invalid, "underfunded caller accepted at {upgrade:?}");
    }
}

#[test]
fn fee_cap_below_base_fee_is_rejected() {
    let fixture = Fixture::new(Kind::Eip1559 { max_fee: 100, priority_fee: 1 });
    for upgrade in UPGRADES {
        let (evm2, revm) = fixture.outcomes(Env::new(upgrade).with_basefee(BASEFEE));
        assert_eq!(revm, Outcome::Invalid);
        assert_eq!(evm2, Outcome::Invalid);
    }
}

#[test]
fn tx_gas_limit_cap_from_azul() {
    let mut fixture = Fixture::eip1559();
    fixture.gas_limit = (1 << 24) + 1;
    for upgrade in [BaseUpgrade::Jovian, BaseUpgrade::Azul] {
        let (evm2, revm) = fixture.outcomes(Env::new(upgrade).with_basefee(BASEFEE));
        assert_eq!(evm2, revm, "{upgrade:?}");
    }
    let (evm2, _) = fixture.outcomes(Env::new(BaseUpgrade::Azul).with_basefee(BASEFEE));
    assert_eq!(evm2, Outcome::Invalid);
}

#[test]
fn l1_attributes_deposit_updates_fees_for_later_transactions() {
    // An L1-attributes-style deposit rewrites the L1 base fee slot of the `L1Block` predeploy;
    // the standard transaction that follows in the same block must price its L1 fee from the
    // updated value on both engines.
    // PUSH32 <fee> PUSH1 0x01 SSTORE STOP.
    let mut code = vec![0x7f];
    code.extend_from_slice(&U256::from(90_000_000_000u64).to_be_bytes::<32>());
    code.extend_from_slice(&[0x60, 0x01, 0x55, 0x00]);
    let depositor = SystemAddresses::DEPOSITOR_ACCOUNT;
    let fixture = Fixture::eip1559();
    let mut seeds = fixture.seeds.clone();
    seeds.push(Seed::contract(Predeploys::L1_BLOCK_INFO, code));
    let mut compare = fixture.compare.clone();
    compare.push((Predeploys::L1_BLOCK_INFO, vec![U256::from(1)]));

    let deposit = TxDeposit {
        source_hash: B256::repeat_byte(1),
        from: depositor,
        to: TxKind::Call(Predeploys::L1_BLOCK_INFO),
        gas_limit: 1_000_000,
        is_system_transaction: false,
        ..Default::default()
    };
    let revm_deposit = BaseTransaction::builder()
        .base(
            TxEnv::builder()
                .caller(depositor)
                .kind(TxKind::Call(Predeploys::L1_BLOCK_INFO))
                .gas_limit(1_000_000),
        )
        .source_hash(B256::repeat_byte(1))
        .build_fill();

    for upgrade in UPGRADES {
        let env = Env::new(upgrade).with_basefee(BASEFEE);
        let evm2 = run_evm2_seq(
            env,
            &seeds,
            vec![(deposit.clone().into(), depositor), (fixture.evm2_tx(), SENDER)],
            &compare,
        );
        let revm =
            run_revm_seq(env, &seeds, vec![revm_deposit.clone(), fixture.revm_tx()], &compare);
        assert_eq!(evm2, revm, "diverged at {upgrade:?}");

        // The fee must reflect the updated L1 base fee, not the seeded one.
        let single = run_evm2(env, &fixture.seeds, fixture.evm2_tx(), SENDER, &compare);
        let (Outcome::Executed { accounts: after, .. }, Outcome::Executed { accounts: before, .. }) =
            (evm2, single)
        else {
            panic!("transaction rejected");
        };
        assert!(
            after[&Predeploys::L1_FEE_VAULT].balance > before[&Predeploys::L1_FEE_VAULT].balance,
            "L1 fee did not use the updated L1 base fee at {upgrade:?}"
        );
    }
}

/// Deliberate divergence: with fee charging on and an empty L1-fee payload, the reference charges
/// no operator fee (the payload is fee-exempt) but still refunds the operator fee on the unused
/// gas, crediting the caller with funds it never paid. This port refunds only what was charged.
#[test]
fn empty_payload_does_not_refund_uncharged_operator_fee() {
    let mut fixture = Fixture::eip1559();
    fixture.enveloped = false;
    for upgrade in [BaseUpgrade::Isthmus, BaseUpgrade::Jovian, BaseUpgrade::Azul] {
        let (evm2, revm) = fixture.outcomes(Env::new(upgrade).with_basefee(BASEFEE));
        let (
            Outcome::Executed { txs: evm2_txs, accounts: evm2_accounts },
            Outcome::Executed { txs: revm_txs, accounts: revm_accounts },
        ) = (evm2, revm)
        else {
            panic!("transaction rejected");
        };
        assert_eq!(evm2_txs, revm_txs);
        let spec = base_evm2::BaseSpecId::new(upgrade);
        let storage = common::l1_block_storage();
        let info = base_evm2::L1BlockInfo::try_fetch_with(
            |slot| {
                Ok::<_, ()>(
                    storage.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v).unwrap_or_default(),
                )
            },
            U256::from(common::BLOCK_NUMBER),
            spec,
        )
        .unwrap();
        let params = info.params();
        let used = evm2_txs[0].gas_used;
        let phantom_refund = params
            .operator_fee_charge_inner(U256::from(fixture.gas_limit), upgrade)
            .saturating_sub(params.operator_fee_charge_inner(U256::from(used), upgrade));
        assert!(!phantom_refund.is_zero());
        for (address, state) in &evm2_accounts {
            let reference = &revm_accounts[address];
            if *address == SENDER {
                assert_eq!(state.balance + phantom_refund, reference.balance, "{upgrade:?}");
            } else {
                assert_eq!(state, reference, "{address} at {upgrade:?}");
            }
        }
    }
}
