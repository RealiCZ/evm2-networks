//! Differential parity tests for deposit execution.
//!
//! Runs the same deposit through `base-evm2` and the revm-based `base-common-evm` reference and
//! asserts the two engines agree on status, gas used, failed-deposit classification, created
//! address, output, logs, and the post-state of every account the fixture cares about.
//!
//! Adapted from the Base team's `base-common-evm2` `deposit_parity` harness.

mod common;

use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use base_common_consensus::TxDeposit;
use base_common_evm::BaseTransaction;
use base_common_genesis::BaseUpgrade;
use base_evm2::BaseTxEnvelope;
use common::{Compare, Env, Seed, run_evm2, run_revm};
use revm::context::TxEnv;

const SENDER: Address = Address::repeat_byte(0x11);
const TARGET: Address = Address::repeat_byte(0x22);
const SOURCE_HASH: B256 = B256::repeat_byte(0xab);

/// Upgrades swept by every fixture: MERGE, SHANGHAI, CANCUN, PRAGUE, and OSAKA gas schedules.
const UPGRADES: [BaseUpgrade; 6] = [
    BaseUpgrade::Regolith,
    BaseUpgrade::Canyon,
    BaseUpgrade::Ecotone,
    BaseUpgrade::Isthmus,
    BaseUpgrade::Azul,
    BaseUpgrade::Beryl,
];

struct Fixture {
    seeds: Vec<Seed>,
    to: TxKind,
    mint: u128,
    value: U256,
    gas_limit: u64,
    input: Vec<u8>,
    is_system: bool,
    compare: Compare,
}

impl Fixture {
    fn call(to: Address) -> Self {
        Self {
            seeds: vec![],
            to: TxKind::Call(to),
            mint: 1_000,
            value: U256::ZERO,
            gas_limit: 200_000,
            input: vec![],
            is_system: false,
            compare: vec![(SENDER, vec![]), (to, vec![])],
        }
    }

    fn create(init: Vec<u8>) -> Self {
        Self {
            to: TxKind::Create,
            input: init,
            compare: vec![(SENDER, vec![]), (SENDER.create(0), vec![])],
            ..Self::call(TARGET)
        }
    }

    fn evm2_tx(&self) -> BaseTxEnvelope {
        BaseTxEnvelope::from(TxDeposit {
            source_hash: SOURCE_HASH,
            from: SENDER,
            to: self.to,
            mint: self.mint,
            value: self.value,
            gas_limit: self.gas_limit,
            is_system_transaction: self.is_system,
            input: Bytes::from(self.input.clone()),
        })
    }

    fn revm_tx(&self) -> BaseTransaction<TxEnv> {
        let mut builder = BaseTransaction::builder()
            .base(
                TxEnv::builder()
                    .caller(SENDER)
                    .kind(self.to)
                    .gas_limit(self.gas_limit)
                    .value(self.value)
                    .data(Bytes::from(self.input.clone())),
            )
            .source_hash(SOURCE_HASH)
            .mint(self.mint);
        if self.is_system {
            builder = builder.is_system_transaction();
        }
        builder.build_fill()
    }

    fn assert_parity_with(&self, env: impl Fn(BaseUpgrade) -> Env) {
        for upgrade in UPGRADES {
            let env = env(upgrade);
            let evm2 = run_evm2(env, &self.seeds, self.evm2_tx(), SENDER, &self.compare);
            let revm = run_revm(env, &self.seeds, self.revm_tx(), &self.compare);
            assert_eq!(evm2, revm, "deposit outcomes diverged at {upgrade:?}");
        }
    }

    fn assert_parity(&self) {
        // A non-zero base fee and coinbase prove deposits pay no fees and credit no vaults.
        self.assert_parity_with(|upgrade| Env::new(upgrade).with_basefee(7));
    }
}

#[test]
fn mint_only_to_eoa() {
    Fixture::call(TARGET).assert_parity();
}

#[test]
fn mint_and_value_transfer() {
    Fixture { value: U256::from(300), ..Fixture::call(TARGET) }.assert_parity();
}

#[test]
fn value_above_balance_is_failed() {
    Fixture { value: U256::from(5_000), ..Fixture::call(TARGET) }.assert_parity();
}

#[test]
fn value_above_balance_with_balance_check_disabled() {
    // With balance checks disabled the reference raises the sender balance to the value.
    Fixture { value: U256::from(5_000), ..Fixture::call(TARGET) }.assert_parity_with(|upgrade| {
        let mut env = Env::new(upgrade);
        env.balance_check = false;
        env
    });
}

#[test]
fn existing_sender_balance_and_nonce() {
    Fixture {
        seeds: vec![Seed { nonce: 7, ..Seed::eoa(SENDER, U256::from(10_000)) }],
        value: U256::from(10_500),
        ..Fixture::call(TARGET)
    }
    .assert_parity();
}

#[test]
fn call_contract_with_sstore_and_log() {
    // PUSH1 0x2a PUSH1 0x00 SSTORE PUSH1 0x00 PUSH1 0x00 LOG0 STOP.
    let code = vec![0x60, 0x2a, 0x60, 0x00, 0x55, 0x60, 0x00, 0x60, 0x00, 0xa0, 0x00];
    Fixture {
        seeds: vec![Seed::contract(TARGET, code)],
        value: U256::from(50),
        compare: vec![(SENDER, vec![]), (TARGET, vec![U256::ZERO])],
        ..Fixture::call(TARGET)
    }
    .assert_parity();
}

#[test]
fn sstore_clear_refund() {
    // PUSH1 0x00 PUSH1 0x00 SSTORE STOP, clearing a non-zero slot to earn a refund.
    let code = vec![0x60, 0x00, 0x60, 0x00, 0x55, 0x00];
    Fixture {
        seeds: vec![Seed::contract(TARGET, code).with_storage(vec![(U256::ZERO, U256::from(1))])],
        compare: vec![(SENDER, vec![]), (TARGET, vec![U256::ZERO])],
        ..Fixture::call(TARGET)
    }
    .assert_parity();
}

#[test]
fn create_deposit() {
    // PUSH1 0x00 PUSH1 0x00 RETURN: deploys empty runtime code.
    Fixture::create(vec![0x60, 0x00, 0x60, 0x00, 0xf3]).assert_parity();
}

#[test]
fn create_deposit_with_existing_nonce() {
    let mut fixture = Fixture::create(vec![0x60, 0x00, 0x60, 0x00, 0xf3]);
    fixture.seeds = vec![Seed { nonce: 3, ..Seed::eoa(SENDER, U256::ZERO) }];
    fixture.compare = vec![(SENDER, vec![]), (SENDER.create(3), vec![])];
    fixture.assert_parity();
}

#[test]
fn reverted_deposit() {
    // PUSH1 0x00 PUSH1 0x00 REVERT.
    Fixture::create(vec![0x60, 0x00, 0x60, 0x00, 0xfd]).assert_parity();
}

#[test]
fn halted_deposit_is_failed() {
    // INVALID.
    Fixture::create(vec![0xfe]).assert_parity();
}

#[test]
fn halted_call_with_value_rolls_back_transfer() {
    Fixture {
        seeds: vec![Seed::contract(TARGET, vec![0xfe])],
        value: U256::from(500),
        ..Fixture::call(TARGET)
    }
    .assert_parity();
}

#[test]
fn halted_call_with_balance_check_disabled_drops_top_up() {
    // The top-up to the value is part of the deposit's state changes and is reverted with them.
    Fixture {
        seeds: vec![Seed::contract(TARGET, vec![0xfe])],
        value: U256::from(5_000),
        ..Fixture::call(TARGET)
    }
    .assert_parity_with(|upgrade| {
        let mut env = Env::new(upgrade);
        env.balance_check = false;
        env
    });
}

#[test]
fn system_transaction_is_failed() {
    Fixture { is_system: true, ..Fixture::call(TARGET) }.assert_parity();
}

#[test]
fn gas_limit_below_intrinsic_is_failed() {
    Fixture { gas_limit: 20_999, value: U256::from(1), ..Fixture::call(TARGET) }.assert_parity();
}

#[test]
fn gas_limit_below_calldata_floor_is_failed_from_isthmus() {
    // 1000 non-zero bytes: intrinsic 21000 + 16000 = 37000, EIP-7623 floor 21000 + 40000 = 61000.
    Fixture { gas_limit: 50_000, input: vec![0x01; 1_000], ..Fixture::call(TARGET) }
        .assert_parity();
}

#[test]
fn calldata_floor_applies_to_gas_used() {
    Fixture { gas_limit: 100_000, input: vec![0x01; 1_000], ..Fixture::call(TARGET) }
        .assert_parity();
}

#[test]
fn deposit_ignores_tx_gas_limit_cap() {
    // Above the Osaka EIP-7825 cap of 2^24: deposits skip environment validation.
    Fixture { gas_limit: 20_000_000, ..Fixture::call(TARGET) }.assert_parity();
}

#[test]
fn warm_account_access() {
    // PUSH1 0x01 EXTCODESIZE POP COINBASE EXTCODESIZE POP STOP: precompile and coinbase warmth.
    let code = vec![0x60, 0x01, 0x3b, 0x50, 0x41, 0x3b, 0x50, 0x00];
    Fixture { seeds: vec![Seed::contract(TARGET, code)], ..Fixture::call(TARGET) }.assert_parity();
}
