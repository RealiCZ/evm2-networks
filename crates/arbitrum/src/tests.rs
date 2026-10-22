//! End-to-end execution tests.
//!
//! Expected gas and balances are derived from Nitro's formulas (`arbos/tx_processor.go`,
//! `core/state_transition.go`, `precompiles/`), not from this implementation.

use crate::{
    ARB_SYS_ADDRESS, ARBITRUM_ONE_CHAIN_ID, ArbBlockEnv, ArbBlockEnvExt, ArbEvmFactory,
    ArbOsVersion, ArbTxEnvelope, BATCH_POSTER_ADDRESS, L1_PRICER_FUNDS_POOL_ADDRESS,
    state::{
        ArbOsStorage, blockhashes_storage, features_storage, l1_pricing, l1_pricing_storage,
        l2_pricing, l2_pricing_storage, root, vector_element,
    },
};
use alloy_consensus::{TxEip1559, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, TxKind, U256, address, hex};
use evm2::{
    TxResult,
    ethereum::TxEnvelope,
    evm::{AccountInfo, InMemoryDB},
    interpreter::InstrStop,
    registry::HandlerError,
};

const CALLER: Address = address!("0x00000000000000000000000000000000000ca11e");
const CONTRACT: Address = address!("0x000000000000000000000000000000000000c0de");
const NETWORK_FEE_ACCOUNT: Address = address!("0x00000000000000000000000000000000000000ee");
const INFRA_FEE_ACCOUNT: Address = address!("0x00000000000000000000000000000000000000ef");
const CALLER_BALANCE: u64 = 1_000_000_000_000;
const BASEFEE: u64 = 10;
const MIN_BASE_FEE: u64 = 4;
const L2_BLOCK: u64 = 400_000_000;
const L1_BLOCK: u64 = 24_000_000;

/// Transaction and environment for one execution.
struct Run {
    spec: ArbOsVersion,
    coinbase: Address,
    tx: TxEnvelope,
    poster_gas: u64,
    code: Vec<u8>,
    storage: Vec<(U256, U256)>,
    arbos: Vec<(ArbOsStorage, u64, U256)>,
    l1_block_number: Option<u64>,
}

impl Run {
    fn new(spec: ArbOsVersion) -> Self {
        Self {
            spec,
            coinbase: BATCH_POSTER_ADDRESS,
            tx: legacy(BASEFEE as u128, 1_000_000, TxKind::Call(CONTRACT), Vec::new()),
            poster_gas: 0,
            code: Vec::new(),
            storage: Vec::new(),
            arbos: vec![
                (
                    ArbOsStorage::ROOT,
                    root::NETWORK_FEE_ACCOUNT,
                    NETWORK_FEE_ACCOUNT.into_word().into(),
                ),
                (ArbOsStorage::ROOT, root::INFRA_FEE_ACCOUNT, INFRA_FEE_ACCOUNT.into_word().into()),
                (l2_pricing_storage(), l2_pricing::MIN_BASE_FEE_WEI, U256::from(MIN_BASE_FEE)),
                (l2_pricing_storage(), l2_pricing::PER_BLOCK_GAS_LIMIT, U256::from(32_000_000)),
                (l2_pricing_storage(), l2_pricing::PER_TX_GAS_LIMIT, U256::from(32_000_000)),
            ],
            l1_block_number: Some(L1_BLOCK),
        }
    }

    fn code(mut self, code: &[u8]) -> Self {
        self.code = code.to_vec();
        self
    }

    fn tx(mut self, tx: TxEnvelope) -> Self {
        self.tx = tx;
        self
    }

    fn arbos(mut self, storage: ArbOsStorage, offset: u64, value: u64) -> Self {
        self.arbos.push((storage, offset, U256::from(value)));
        self
    }

    /// Executes the transaction and returns its result and the balances of `watch` and the ArbOS
    /// state values at `slots` afterwards.
    fn exec(
        &self,
        watch: &[Address],
        slots: &[(ArbOsStorage, u64)],
    ) -> (Result<TxResult<crate::ArbEvmTypes>, HandlerError>, Vec<U256>, Vec<U256>) {
        let mut db = InMemoryDB::default();
        db.insert_account_info(
            &CALLER,
            AccountInfo::default().with_balance(U256::from(CALLER_BALANCE)),
        );
        db.insert_account_info(
            &CONTRACT,
            AccountInfo::default()
                .with_code(evm2::bytecode::Bytecode::new_raw(self.code.clone().into())),
        );
        for (key, value) in &self.storage {
            db.insert_account_storage(&CONTRACT, key, value);
        }
        // ArbOS creates the account holding its state with nonce 1.
        db.insert_account_info(&crate::ARBOS_STATE_ADDRESS, AccountInfo::default().with_nonce(1));
        for (storage, offset, value) in &self.arbos {
            db.insert_account_storage(
                &crate::ARBOS_STATE_ADDRESS,
                &storage.slot_u64(*offset),
                value,
            );
        }

        let factory = ArbEvmFactory;
        let block = ArbBlockEnv {
            number: U256::from(L2_BLOCK),
            beneficiary: self.coinbase,
            basefee: U256::from(BASEFEE),
            gas_limit: U256::from(1u64 << 50),
            prevrandao: U256::from(1),
            ext: ArbBlockEnvExt { l1_block_number: self.l1_block_number },
            ..Default::default()
        };
        let version = factory.version(self.spec, ARBITRUM_ONE_CHAIN_ID);
        let mut evm = factory.create_evm(self.spec, version, block, db);
        let tx = ArbTxEnvelope::new(self.tx.clone()).with_poster_gas(self.poster_gas);
        let result = evm.transact(&Recovered::new_unchecked(tx, CALLER)).map(|tx| tx.commit());
        let balances = watch
            .iter()
            .map(|address| {
                evm.read_account_info(address).unwrap().map_or(U256::ZERO, |info| info.balance)
            })
            .collect();
        let values = slots
            .iter()
            .map(|(storage, offset)| storage.get(evm.state_mut(), *offset).unwrap())
            .collect();
        (result, balances, values)
    }

    fn result(&self) -> TxResult<crate::ArbEvmTypes> {
        self.exec(&[], &[]).0.unwrap()
    }
}

fn legacy(gas_price: u128, gas_limit: u64, to: TxKind, input: Vec<u8>) -> TxEnvelope {
    TxEnvelope::Legacy(TxLegacy {
        chain_id: Some(ARBITRUM_ONE_CHAIN_ID),
        gas_price,
        gas_limit,
        to,
        input: input.into(),
        ..Default::default()
    })
}

fn eip1559(max_fee: u128, priority_fee: u128) -> TxEnvelope {
    TxEnvelope::Eip1559(TxEip1559 {
        chain_id: ARBITRUM_ONE_CHAIN_ID,
        gas_limit: 1_000_000,
        max_fee_per_gas: max_fee,
        max_priority_fee_per_gas: priority_fee,
        to: TxKind::Call(CONTRACT),
        ..Default::default()
    })
}

/// Opcode bytes used by the test contracts.
mod op {
    pub(super) const GASPRICE: u8 = 0x3a;
    pub(super) const NUMBER: u8 = 0x43;
    pub(super) const MSTORE: u8 = 0x52;
    pub(super) const SSTORE: u8 = 0x55;
    pub(super) const JUMP: u8 = 0x56;
    pub(super) const JUMPDEST: u8 = 0x5b;
    pub(super) const PUSH1: u8 = 0x60;
    pub(super) const RETURN: u8 = 0xf3;
}

/// Returns the word pushed by `opcode`.
fn return_word(opcode: u8) -> [u8; 9] {
    [opcode, op::PUSH1, 0, op::MSTORE, op::PUSH1, 32, op::PUSH1, 0, op::RETURN]
}

fn word(value: u64) -> Bytes {
    Bytes::from(U256::from(value).to_be_bytes::<32>())
}

#[test]
fn fees_are_split_between_network_infra_and_poster() {
    // A plain call to a contract without code, paying a 1,000 gas poster charge.
    let mut run = Run::new(ArbOsVersion::V61);
    run.tx = legacy(30, 100_000, TxKind::Call(CONTRACT), Vec::new());
    run.poster_gas = 1_000;
    let l1 = l1_pricing_storage();
    let l2 = l2_pricing_storage();
    let watch = [CALLER, NETWORK_FEE_ACCOUNT, INFRA_FEE_ACCOUNT, L1_PRICER_FUNDS_POOL_ADDRESS];
    let slots = [(l1, l1_pricing::L1_FEES_AVAILABLE), (l2, l2_pricing::GAS_BACKLOG)];
    let (result, balances, values) = run.exec(&watch, &slots);
    let result = result.unwrap();

    // Tips are dropped, so the transaction pays the base fee.
    assert_eq!(result.tx_gas_used(), 22_000);
    assert_eq!(result.ext.paid_gas_price, U256::from(BASEFEE));
    assert_eq!(result.ext.poster_fee, U256::from(1_000 * BASEFEE));
    let compute_gas = 21_000;
    assert_eq!(
        balances,
        [
            U256::from(CALLER_BALANCE - 22_000 * BASEFEE),
            U256::from(compute_gas * (BASEFEE - MIN_BASE_FEE)),
            U256::from(compute_gas * MIN_BASE_FEE),
            U256::from(1_000 * BASEFEE),
        ]
    );
    // The poster fee is recorded as available L1 fees, and the compute gas grows the backlog.
    assert_eq!(values, [U256::from(1_000 * BASEFEE), U256::from(compute_gas)]);

    // With single-gas pricing constraints, from ArbOS 50, every constraint's backlog grows.
    let constraints = l2.open(&[l2_pricing::GAS_CONSTRAINTS]);
    run = run.arbos(constraints, 0, 2).arbos(vector_element(&constraints, 1), 2, 5);
    let slots = [
        (l2, l2_pricing::GAS_BACKLOG),
        (vector_element(&constraints, 0), l2_pricing::CONSTRAINT_BACKLOG),
        (vector_element(&constraints, 1), l2_pricing::CONSTRAINT_BACKLOG),
    ];
    let (_, _, values) = run.exec(&[], &slots);
    assert_eq!(values, [U256::ZERO, U256::from(compute_gas), U256::from(compute_gas + 5)]);
}

#[test]
fn tips_are_collected_only_when_enabled_for_sequenced_blocks() {
    let code = return_word(op::GASPRICE);
    // `min(30, 10 + 5)`, a tip of 5 per gas over the base fee.
    let run = Run::new(ArbOsVersion::V61).code(&code).tx(eip1559(30, 5));
    let dropped = run.result();
    assert_eq!(dropped.output, word(BASEFEE));
    assert_eq!(dropped.ext.paid_gas_price, U256::from(BASEFEE));

    let collecting = run.arbos(ArbOsStorage::ROOT, root::COLLECT_TIPS, 1);
    let (result, balances, _) = collecting.exec(&[CALLER, NETWORK_FEE_ACCOUNT], &[]);
    let result = result.unwrap();
    let gas_used = result.tx_gas_used();
    assert_eq!(result.output, word(15));
    assert_eq!(result.ext.paid_gas_price, U256::from(15));
    assert_eq!(
        balances,
        [U256::from(CALLER_BALANCE - gas_used * 15), U256::from(gas_used * (15 - MIN_BASE_FEE)),]
    );

    // Delayed-inbox messages, whose coinbase is not the batch poster, never pay tips.
    let mut delayed = collecting;
    delayed.coinbase = address!("0x00000000000000000000000000000000000c0b0b");
    assert_eq!(delayed.result().output, word(BASEFEE));

    // Before ArbOS 3, `GASPRICE` returns the effective gas price even when the tip is dropped.
    let early = Run::new(ArbOsVersion::V2).code(&code).tx(eip1559(30, 5)).result();
    assert_eq!(early.output, word(15));
    assert_eq!(early.ext.paid_gas_price, U256::from(BASEFEE));
}

#[test]
fn priority_fee_is_only_checked_when_the_tip_is_kept() {
    // The fee cap equals the base fee, so the effective price keeps no tip to drop and Nitro
    // checks the priority fee against the fee cap.
    let error = Run::new(ArbOsVersion::V61).tx(eip1559(BASEFEE as u128, 20)).exec(&[], &[]).0;
    assert!(matches!(error, Err(HandlerError::PriorityFeeGreaterThanMaxFee)), "{error:?}");

    // A dropped tip zeroes the priority fee first.
    let result = Run::new(ArbOsVersion::V61).tx(eip1559(15, 20)).result();
    assert!(result.status);
}

#[test]
fn poster_gas_is_not_refundable() {
    // Clears two non-zero slots: 4 PUSH1 and two cold clearing SSTOREs (5,000 each) refunding
    // 4,800 each.
    let code = [op::PUSH1, 0, op::PUSH1, 0, op::SSTORE, op::PUSH1, 0, op::PUSH1, 1, op::SSTORE];
    let mut run = Run::new(ArbOsVersion::V61).code(&code);
    run.storage = vec![(U256::ZERO, U256::from(1)), (U256::from(1), U256::from(1))];
    run.poster_gas = 10_000;
    let spent = 21_000 + 12 + 10_000 + 10_000;
    // The refund is capped at a fifth of the gas used without the poster gas.
    let refund = (spent - 10_000) / 5;
    assert!(refund < 9_600);
    assert_eq!(run.result().tx_gas_used(), spent - refund);
}

#[test]
fn execution_gas_is_capped_by_arbos() {
    // An infinite loop runs out of gas, consuming all execution gas the cap allows.
    let code = [op::JUMPDEST, op::PUSH1, 0, op::JUMP];
    let l2 = l2_pricing_storage();

    // From ArbOS 50 the transaction gas limit, intrinsic gas included, is capped.
    let run =
        Run::new(ArbOsVersion::V61).code(&code).arbos(l2, l2_pricing::PER_TX_GAS_LIMIT, 100_000);
    let (result, balances, _) = run.exec(&[CALLER], &[]);
    let result = result.unwrap();
    assert_eq!(result.stop, InstrStop::OutOfGas);
    assert_eq!(result.tx_gas_used(), 100_000);
    // The held gas is refunded.
    assert_eq!(balances, [U256::from(CALLER_BALANCE - 100_000 * BASEFEE)]);

    // Before, execution gas was capped at the block gas limit.
    let run =
        Run::new(ArbOsVersion::V40).code(&code).arbos(l2, l2_pricing::PER_BLOCK_GAS_LIMIT, 200_000);
    assert_eq!(run.result().tx_gas_used(), 21_000 + 200_000);
}

#[test]
fn calldata_floor_requires_the_arbos_feature() {
    let input = vec![1; 1_000];
    let tx = legacy(BASEFEE as u128, 1_000_000, TxKind::Call(CONTRACT), input);
    let run = Run::new(ArbOsVersion::V40).tx(tx);
    assert_eq!(run.result().tx_gas_used(), 21_000 + 16 * 1_000);

    // EIP-7623: 21,000 plus 10 gas per token, four tokens per non-zero byte.
    let run = run.arbos(features_storage(), 0, 1);
    assert_eq!(run.result().tx_gas_used(), 21_000 + 40 * 1_000);
}

#[test]
fn number_returns_the_l1_block_number() {
    let code = return_word(op::NUMBER);
    let run = Run::new(ArbOsVersion::V61).code(&code);
    assert_eq!(run.result().output, word(L1_BLOCK));

    // Without a header value, the L1 block number is read from the ArbOS state.
    let mut run = run.arbos(blockhashes_storage(), 0, L1_BLOCK + 7);
    run.l1_block_number = None;
    assert_eq!(run.result().output, word(L1_BLOCK + 7));
}

#[test]
fn arb_sys_returns_l2_block_and_version() {
    let call = |selector: [u8; 4]| {
        let tx = legacy(BASEFEE as u128, 100_000, TxKind::Call(ARB_SYS_ADDRESS), selector.to_vec());
        Run::new(ArbOsVersion::V61).tx(tx).result()
    };
    // 4 non-zero calldata bytes, 800 gas to open the ArbOS state and 3 for the result word.
    let block_number = call(hex!("a3b1b31d"));
    assert_eq!(block_number.output, word(L2_BLOCK));
    assert_eq!(block_number.tx_gas_used(), 21_000 + 4 * 16 + 803);
    assert_eq!(call(hex!("051038f2")).output, word(55 + 61));
    assert_eq!(call(hex!("d127f54a")).output, word(ARBITRUM_ONE_CHAIN_ID));

    // An unknown selector reverts and consumes all gas.
    let unknown = call(hex!("deadbeef"));
    assert!(!unknown.status);
    assert_eq!(unknown.tx_gas_used(), 100_000);
}

#[test]
fn unsupported_arbos_precompiles_abort() {
    let retryable = address!("0x000000000000000000000000000000000000006e");
    let tx = legacy(BASEFEE as u128, 100_000, TxKind::Call(retryable), Vec::new());
    let error = Run::new(ArbOsVersion::V61).tx(tx).exec(&[], &[]).0;
    assert!(matches!(error, Err(HandlerError::Fatal(_))), "{error:?}");

    // Stylus programs abort the transaction too.
    let error = Run::new(ArbOsVersion::V61).code(&hex!("eff00000")).exec(&[], &[]).0;
    assert!(matches!(error, Err(HandlerError::Fatal(_))), "{error:?}");
    // Before Stylus, the prefix is an undefined opcode.
    let result = Run::new(ArbOsVersion::V20).code(&hex!("eff00000")).result();
    assert_eq!(result.stop, InstrStop::OpcodeNotFound);
}

#[test]
fn precompile_sets_follow_arbos_versions() {
    let has = |spec, address: u64| {
        crate::arbitrum_precompile_map(spec)
            .contains(&Address::left_padding_from(&address.to_be_bytes()))
    };
    let (kzg, bls_g1_add, p256, arb_wasm, arb_sys) = (0x0a, 0x0b, 0x100, 0x71, 0x64);
    assert!(has(ArbOsVersion::V20, arb_sys));
    assert!(!has(ArbOsVersion::V20, kzg) && !has(ArbOsVersion::V20, p256));
    assert!(!has(ArbOsVersion::V20, arb_wasm));
    // ArbOS 40 runs Prague but keeps the Cancun precompiles.
    assert!(has(ArbOsVersion::V40, kzg) && has(ArbOsVersion::V40, p256));
    assert!(has(ArbOsVersion::V40, arb_wasm));
    assert!(!has(ArbOsVersion::V40, bls_g1_add));
    assert!(has(ArbOsVersion::V50, bls_g1_add) && has(ArbOsVersion::V50, p256));
}
