//! End-to-end handler tests.
//!
//! Expected values follow op-revm's handler semantics and reuse its test vectors (for example the
//! 1048 wei Regolith L1 cost of `0xFACADE` and the Isthmus/Jovian operator fee parameters).

use crate::{
    L1BlockInfo, OpEvm, OpEvmFactory, OpEvmTypes, OpSpecId, OpTxEnvelope, OpTxError, TxDeposit,
    constants::*, op_tx_registry, op_version,
};
use alloy_consensus::{TxEip1559, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, TxKind, U256, bytes};
use evm2::{
    EvmFeatures, TxResult,
    bytecode::Bytecode,
    env::{BlockEnv, BlockEnvExt},
    evm::{AccountInfo, InMemoryDB},
    interpreter::{InstrStop, op},
    registry::HandlerError,
};

const CALLER: Address = Address::with_last_byte(0xaa);
const TARGET: Address = Address::with_last_byte(0xbb);
const COINBASE: Address = Address::with_last_byte(0xcc);
/// Contract whose code is the designated invalid opcode.
const INVALID: Address = Address::with_last_byte(0xdd);
/// Contract that always reverts.
const REVERTER: Address = Address::with_last_byte(0xee);

fn db_with_caller(balance: u64) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&CALLER, AccountInfo::default().with_balance(U256::from(balance)));
    db.insert_account_info(
        &INVALID,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[op::INVALID]))),
    );
    db.insert_account_info(
        &REVERTER,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[
            op::PUSH1,
            0,
            op::PUSH1,
            0,
            op::REVERT,
        ]))),
    );
    db
}

fn block(number: u64, basefee: u64) -> BlockEnv<OpEvmTypes> {
    BlockEnvExt {
        number: U256::from(number),
        basefee: U256::from(basefee),
        beneficiary: COINBASE,
        ..Default::default()
    }
}

fn evm_with(
    spec: OpSpecId,
    db: InMemoryDB,
    block: BlockEnv<OpEvmTypes>,
    l1: Option<L1BlockInfo>,
    disable: EvmFeatures,
) -> OpEvm<'static> {
    let mut version = op_version(spec, 10);
    version.features.remove(disable);
    let mut evm = OpEvmFactory.create_evm(spec, version, block, db);
    if let Some(l1) = l1 {
        evm.ext_mut().l1_block_info = L1BlockInfo { l2_block: Some(block.number), ..l1 };
    }
    evm
}

fn new_evm(
    spec: OpSpecId,
    db: InMemoryDB,
    block: BlockEnv<OpEvmTypes>,
    l1: L1BlockInfo,
) -> OpEvm<'static> {
    evm_with(spec, db, block, Some(l1), EvmFeatures::empty())
}

fn regolith_l1() -> L1BlockInfo {
    L1BlockInfo {
        l1_base_fee: U256::from(1_000),
        l1_fee_overhead: Some(U256::from(1_000)),
        l1_base_fee_scalar: U256::from(1_000),
        ..Default::default()
    }
}

fn legacy(
    gas_limit: u64,
    gas_price: u128,
    to: Address,
    enveloped: Option<Bytes>,
) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy { gas_limit, gas_price, to: TxKind::Call(to), ..Default::default() };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, enveloped), CALLER)
}

fn deposit(tx: TxDeposit) -> Recovered<OpTxEnvelope> {
    let from = tx.from;
    Recovered::new_unchecked(OpTxEnvelope::deposit(tx), from)
}

fn balance(evm: &mut OpEvm<'_>, address: Address) -> U256 {
    evm.state_mut().account_info_untracked(&address).unwrap().map(|a| a.balance).unwrap_or_default()
}

fn nonce(evm: &mut OpEvm<'_>, address: Address) -> u64 {
    evm.state_mut().account_info_untracked(&address).unwrap().map(|a| a.nonce).unwrap_or_default()
}

fn transact(
    evm: &mut OpEvm<'_>,
    tx: &Recovered<OpTxEnvelope>,
) -> Result<TxResult<OpEvmTypes>, HandlerError> {
    evm.transact(tx).map(|executed| executed.commit())
}

#[test]
fn regolith_l1_fee_is_charged_and_vaults_credited() {
    // gas: 21000 used of 30000 at price 3, base fee 2. L1 cost of 0xFACADE is 1048 (op-revm).
    let initial = 30_000 * 3 + 1_048 + 5;
    let mut evm = new_evm(OpSpecId::REGOLITH, db_with_caller(initial), block(1, 2), regolith_l1());
    let result = transact(&mut evm, &legacy(30_000, 3, TARGET, Some(bytes!("FACADE")))).unwrap();

    assert!(result.status);
    assert_eq!(result.tx_gas_used(), 21_000);
    assert_eq!(result.ext.l1_fee, U256::from(1_048));
    assert!(!result.ext.failed_deposit);
    assert_eq!(balance(&mut evm, CALLER), U256::from(initial - 21_000 * 3 - 1_048));
    assert_eq!(balance(&mut evm, COINBASE), U256::from(21_000));
    assert_eq!(balance(&mut evm, BASE_FEE_RECIPIENT), U256::from(42_000));
    assert_eq!(balance(&mut evm, L1_FEE_RECIPIENT), U256::from(1_048));
    assert_eq!(balance(&mut evm, OPERATOR_FEE_RECIPIENT), U256::ZERO);
    assert_eq!(nonce(&mut evm, CALLER), 1);
    // The per-transaction L1 cost cache is cleared after settlement.
    assert_eq!(evm.ext().l1_block_info.tx_l1_cost, None);
}

#[test]
fn balance_check_includes_l1_fee() {
    // op-revm `test_remove_l1_cost_lack_of_funds`: the caller must also cover the L1 cost.
    let max_gas_cost = 30_000 * 3;
    let tx = legacy(30_000, 3, TARGET, Some(bytes!("FACADE")));
    let mut evm = new_evm(
        OpSpecId::REGOLITH,
        db_with_caller(max_gas_cost + 1_047),
        block(1, 2),
        regolith_l1(),
    );
    assert_eq!(evm.validate_tx(&tx), Err(HandlerError::InsufficientFunds));
    assert_eq!(transact(&mut evm, &tx), Err(HandlerError::InsufficientFunds));
    assert_eq!(nonce(&mut evm, CALLER), 0);

    let mut evm = new_evm(
        OpSpecId::REGOLITH,
        db_with_caller(max_gas_cost + 1_048),
        block(1, 2),
        regolith_l1(),
    );
    assert!(transact(&mut evm, &tx).unwrap().status);
}

#[test]
fn isthmus_operator_fee_charge_and_refund() {
    // op-revm `test_remove_operator_cost_isthmus` parameters: scalar 10_000_000, constant 50.
    // Charged on the gas limit: 10_000_000 * 100_000 / 1e6 + 50 = 1_000_050. Kept on gas used:
    // 10_000_000 * 21_000 / 1e6 + 50 = 210_050. The difference is refunded.
    let l1 = L1BlockInfo {
        operator_fee_scalar: Some(U256::from(10_000_000)),
        operator_fee_constant: Some(U256::from(50)),
        l1_blob_base_fee: Some(U256::ZERO),
        l1_blob_base_fee_scalar: Some(U256::ZERO),
        ..Default::default()
    };
    let tx = legacy(100_000, 0, TARGET, Some(bytes!("FACADE")));

    let mut evm = new_evm(OpSpecId::ISTHMUS, db_with_caller(1_000_049), block(1, 0), l1);
    assert_eq!(transact(&mut evm, &tx), Err(HandlerError::InsufficientFunds));

    let mut evm = new_evm(OpSpecId::ISTHMUS, db_with_caller(2_000_000), block(1, 0), l1);
    let result = transact(&mut evm, &tx).unwrap();
    assert_eq!(result.ext.operator_fee, U256::from(210_050));
    assert_eq!(result.ext.l1_fee, U256::ZERO);
    assert_eq!(balance(&mut evm, CALLER), U256::from(2_000_000 - 210_050));
    assert_eq!(balance(&mut evm, OPERATOR_FEE_RECIPIENT), U256::from(210_050));
}

#[test]
fn jovian_operator_fee_charge_and_refund() {
    // op-revm `test_remove_operator_cost_jovian` parameters: scalar 2, constant 50, so the fee is
    // `gas * 2 * 100 + 50`: 20_000_050 on the limit, 4_200_050 on gas used.
    let l1 = L1BlockInfo {
        operator_fee_scalar: Some(U256::from(2)),
        operator_fee_constant: Some(U256::from(50)),
        ..Default::default()
    };
    let tx = legacy(100_000, 0, TARGET, Some(bytes!("FACADE")));

    let mut evm = new_evm(OpSpecId::JOVIAN, db_with_caller(20_000_049), block(1, 0), l1);
    assert_eq!(transact(&mut evm, &tx), Err(HandlerError::InsufficientFunds));

    let mut evm = new_evm(OpSpecId::JOVIAN, db_with_caller(20_000_050), block(1, 0), l1);
    let result = transact(&mut evm, &tx).unwrap();
    assert_eq!(result.ext.operator_fee, U256::from(4_200_050));
    assert_eq!(balance(&mut evm, CALLER), U256::from(20_000_050 - 4_200_050));
    assert_eq!(balance(&mut evm, OPERATOR_FEE_RECIPIENT), U256::from(4_200_050));
}

#[test]
fn operator_fee_is_not_charged_before_isthmus() {
    let l1 = L1BlockInfo {
        operator_fee_scalar: Some(U256::from(2)),
        operator_fee_constant: Some(U256::from(50)),
        ..Default::default()
    };
    let mut evm = new_evm(OpSpecId::HOLOCENE, db_with_caller(1_000), block(1, 0), l1);
    let result = transact(&mut evm, &legacy(100_000, 0, TARGET, Some(bytes!("FACADE")))).unwrap();
    assert_eq!(result.ext.operator_fee, U256::ZERO);
    assert_eq!(balance(&mut evm, CALLER), U256::from(1_000));
}

#[test]
fn fee_charge_disabled_skips_l1_and_operator_fees() {
    let l1 = L1BlockInfo {
        operator_fee_scalar: Some(U256::from(2)),
        operator_fee_constant: Some(U256::from(50)),
        ..regolith_l1()
    };
    let mut evm = evm_with(
        OpSpecId::JOVIAN,
        db_with_caller(30_000 * 3),
        block(1, 2),
        Some(l1),
        EvmFeatures::FEE_CHARGE,
    );
    let result = transact(&mut evm, &legacy(30_000, 3, TARGET, Some(bytes!("FACADE")))).unwrap();
    assert!(result.status);
    assert_eq!(balance(&mut evm, CALLER), U256::from(30_000 * 3));
    assert_eq!(balance(&mut evm, COINBASE), U256::ZERO);
    // op-revm's `reward_beneficiary` credits the vaults even without fee charging.
    assert_eq!(balance(&mut evm, BASE_FEE_RECIPIENT), U256::from(42_000));
    assert_eq!(balance(&mut evm, OPERATOR_FEE_RECIPIENT), U256::from(21_000 * 200 + 50));
}

#[test]
fn balance_top_up_covers_l1_fee_when_balance_check_disabled() {
    let mut evm = evm_with(
        OpSpecId::REGOLITH,
        db_with_caller(0),
        block(1, 2),
        Some(regolith_l1()),
        EvmFeatures::BALANCE_CHECK,
    );
    let result = transact(&mut evm, &legacy(30_000, 3, TARGET, Some(bytes!("FACADE")))).unwrap();
    assert!(result.status);
    // Topped up to `gas_limit * price + l1_cost`, then charged gas used and the L1 fee.
    assert_eq!(balance(&mut evm, CALLER), U256::from(9_000 * 3));
    assert_eq!(balance(&mut evm, L1_FEE_RECIPIENT), U256::from(1_048));
}

#[test]
fn missing_enveloped_tx_is_rejected() {
    let mut evm =
        new_evm(OpSpecId::JOVIAN, db_with_caller(1_000_000), block(1, 0), L1BlockInfo::default());
    let err = transact(&mut evm, &legacy(30_000, 0, TARGET, None)).unwrap_err();
    assert_eq!(err.external_ref::<OpTxError>(), Some(&OpTxError::MissingEnvelopedTx));
    // An empty envelope pays no L1 fee.
    assert!(transact(&mut evm, &legacy(30_000, 0, TARGET, Some(Bytes::new()))).unwrap().status);
}

#[test]
fn validation_does_not_leak_l1_cost_cache() {
    let mut evm =
        new_evm(OpSpecId::REGOLITH, db_with_caller(10_000_000), block(1, 0), regolith_l1());
    evm.validate_tx(&legacy(30_000, 0, TARGET, Some(bytes!("FACADE")))).unwrap();
    // `0xFA00CA00DE`: (3*16 + 2*4 + 1000) * 1000 * 1000 / 1e6 = 1056.
    let result =
        transact(&mut evm, &legacy(30_000, 0, TARGET, Some(bytes!("FA00CA00DE")))).unwrap();
    assert_eq!(result.ext.l1_fee, U256::from(1_056));
}

#[test]
fn l1_block_info_is_reloaded_from_storage() {
    // op-revm `test_parse_da_footprint_gas_scalar_jovian` storage layout.
    let block_num = 100;
    let mut packed = [0u8; 32];
    packed[31] = 6; // operator fee constant
    packed[23] = 5; // operator fee scalar
    packed[19] = 7; // DA footprint gas scalar
    let l1_fee_scalars = U256::from_limbs([0, (3 << (64 - BASE_FEE_SCALAR_OFFSET * 2)) | 4, 0, 0]);

    let mut db = db_with_caller(100_000_000);
    for (slot, value) in [
        (L1_BASE_FEE_SLOT, U256::from(1)),
        (ECOTONE_L1_BLOB_BASE_FEE_SLOT, U256::from(2)),
        (ECOTONE_L1_FEE_SCALARS_SLOT, l1_fee_scalars),
        (OPERATOR_FEE_SCALARS_SLOT, U256::from_be_bytes(packed)),
    ] {
        db.insert_account_storage(&L1_BLOCK_CONTRACT, &slot, &value);
    }
    let stale = L1BlockInfo {
        l2_block: Some(U256::from(block_num + 1)),
        operator_fee_scalar: Some(U256::from(2)),
        operator_fee_constant: Some(U256::from(50)),
        ..Default::default()
    };
    let mut evm = evm_with(OpSpecId::JOVIAN, db, block(block_num, 0), None, EvmFeatures::empty());
    evm.ext_mut().l1_block_info = stale;

    let result = transact(&mut evm, &legacy(30_000, 0, TARGET, Some(bytes!("FACADE")))).unwrap();
    assert_eq!(
        evm.ext().l1_block_info,
        L1BlockInfo {
            l2_block: Some(U256::from(block_num)),
            l1_base_fee: U256::from(1),
            l1_base_fee_scalar: U256::from(3),
            l1_blob_base_fee: Some(U256::from(2)),
            l1_blob_base_fee_scalar: Some(U256::from(4)),
            empty_ecotone_scalars: false,
            l1_fee_overhead: None,
            operator_fee_scalar: Some(U256::from(5)),
            operator_fee_constant: Some(U256::from(6)),
            tx_l1_cost: None,
            da_footprint_gas_scalar: Some(7),
        }
    );
    // Jovian operator fee with the reloaded parameters: 21_000 * 5 * 100 + 6.
    assert_eq!(result.ext.operator_fee, U256::from(10_500_006));
}

#[test]
fn regolith_deposit_mints_without_fees() {
    // op-revm `test_commit_mint_value`: 1000 + mint 10.
    let mut evm = new_evm(OpSpecId::REGOLITH, db_with_caller(1_000), block(1, 7), regolith_l1());
    let tx = deposit(TxDeposit {
        from: CALLER,
        to: TxKind::Call(TARGET),
        mint: 10,
        value: U256::from(4),
        gas_limit: 50_000,
        ..Default::default()
    });
    let result = transact(&mut evm, &tx).unwrap();
    assert!(result.status);
    assert_eq!(result.tx_gas_used(), 21_000);
    assert_eq!(balance(&mut evm, CALLER), U256::from(1_006));
    assert_eq!(balance(&mut evm, TARGET), U256::from(4));
    assert_eq!(nonce(&mut evm, CALLER), 1);
    for vault in [BASE_FEE_RECIPIENT, L1_FEE_RECIPIENT, OPERATOR_FEE_RECIPIENT, COINBASE] {
        assert_eq!(balance(&mut evm, vault), U256::ZERO);
    }
}

#[test]
fn regolith_halted_deposit_keeps_only_nonce_and_mint() {
    let mut evm = new_evm(OpSpecId::REGOLITH, db_with_caller(1_000), block(1, 0), regolith_l1());
    let tx = deposit(TxDeposit {
        from: CALLER,
        to: TxKind::Call(INVALID),
        mint: 10,
        value: U256::from(500),
        gas_limit: 50_000,
        ..Default::default()
    });
    let result = transact(&mut evm, &tx).unwrap();
    assert!(!result.status);
    assert!(result.ext.failed_deposit);
    assert_eq!(result.ext.deposit_error, None);
    assert_eq!(result.stop, InstrStop::InvalidFEOpcode);
    assert_eq!(result.tx_gas_used(), 50_000);
    assert_eq!(balance(&mut evm, CALLER), U256::from(1_010));
    assert_eq!(balance(&mut evm, INVALID), U256::ZERO);
    assert_eq!(nonce(&mut evm, CALLER), 1);
}

#[test]
fn regolith_reverted_deposit_reports_actual_gas() {
    let mut evm = new_evm(OpSpecId::REGOLITH, db_with_caller(1_000), block(1, 0), regolith_l1());
    let tx = deposit(TxDeposit {
        from: CALLER,
        to: TxKind::Call(REVERTER),
        mint: 10,
        gas_limit: 50_000,
        ..Default::default()
    });
    let result = transact(&mut evm, &tx).unwrap();
    assert!(!result.status);
    assert!(!result.ext.failed_deposit);
    assert_eq!(result.stop, InstrStop::Revert);
    // 21000 intrinsic + 2 * PUSH1 (PUSH0 does not exist before Canyon).
    assert_eq!(result.tx_gas_used(), 21_006);
    assert_eq!(balance(&mut evm, CALLER), U256::from(1_010));
}

#[test]
fn regolith_system_deposit_and_invalid_deposit_fail() {
    let mut evm = new_evm(OpSpecId::REGOLITH, db_with_caller(1_000), block(1, 0), regolith_l1());
    let system = deposit(TxDeposit {
        from: CALLER,
        to: TxKind::Call(TARGET),
        mint: 10,
        gas_limit: 50_000,
        is_system_transaction: true,
        ..Default::default()
    });
    // Deposits are never invalid: validation passes and execution reports a failed deposit.
    assert_eq!(evm.validate_tx(&system), Ok(()));
    let result = transact(&mut evm, &system).unwrap();
    assert!(result.ext.failed_deposit);
    assert_eq!(
        result.ext.deposit_error.as_ref().and_then(HandlerError::external_ref::<OpTxError>),
        Some(&OpTxError::DepositSystemTxPostRegolith)
    );
    assert_eq!(result.tx_gas_used(), 50_000);
    assert_eq!(balance(&mut evm, CALLER), U256::from(1_010));
    assert_eq!(nonce(&mut evm, CALLER), 1);

    let low_gas = deposit(TxDeposit {
        from: CALLER,
        to: TxKind::Call(TARGET),
        mint: 1,
        gas_limit: 20_000,
        ..Default::default()
    });
    let result = transact(&mut evm, &low_gas).unwrap();
    assert!(result.ext.failed_deposit);
    assert_eq!(
        result.ext.deposit_error,
        Some(HandlerError::IntrinsicGasTooLow { required: 21_000, got: 20_000 })
    );
    assert_eq!(result.tx_gas_used(), 20_000);
    assert_eq!(balance(&mut evm, CALLER), U256::from(1_011));
    assert_eq!(nonce(&mut evm, CALLER), 2);
}

#[test]
fn bedrock_deposit_gas_accounting() {
    let tx = |to, is_system_transaction| {
        deposit(TxDeposit {
            from: CALLER,
            to: TxKind::Call(to),
            mint: 10,
            gas_limit: 50_000,
            is_system_transaction,
            ..Default::default()
        })
    };
    let mut evm = new_evm(OpSpecId::BEDROCK, db_with_caller(1_000), block(1, 0), regolith_l1());

    // Regular deposits report the gas limit, system deposits report zero on success.
    let result = transact(&mut evm, &tx(TARGET, false)).unwrap();
    assert!(result.status);
    assert_eq!(result.tx_gas_used(), 50_000);
    let result = transact(&mut evm, &tx(TARGET, true)).unwrap();
    assert!(result.status);
    assert_eq!(result.tx_gas_used(), 0);

    // A pre-Regolith revert reports the gas limit; a halt is a regular halt, not a failed deposit.
    let result = transact(&mut evm, &tx(REVERTER, false)).unwrap();
    assert_eq!(result.stop, InstrStop::Revert);
    assert_eq!(result.tx_gas_used(), 50_000);
    let result = transact(&mut evm, &tx(INVALID, true)).unwrap();
    assert!(!result.status);
    assert!(!result.ext.failed_deposit);
    assert_eq!(result.tx_gas_used(), 50_000);

    // Pre-Regolith failed (invalid) system deposits report zero gas.
    let low_gas = deposit(TxDeposit {
        from: CALLER,
        to: TxKind::Call(TARGET),
        gas_limit: 20_000,
        is_system_transaction: true,
        ..Default::default()
    });
    let result = transact(&mut evm, &low_gas).unwrap();
    assert!(result.ext.failed_deposit);
    assert_eq!(result.tx_gas_used(), 0);

    assert_eq!(balance(&mut evm, CALLER), U256::from(1_040));
    assert_eq!(nonce(&mut evm, CALLER), 5);
}

#[test]
fn registry_tx_types_per_spec() {
    for spec in OpSpecId::ALL {
        let registry = op_tx_registry(spec);
        for ty in [0, 1, 2, DEPOSIT_TRANSACTION_TYPE] {
            assert!(registry.contains(ty), "{spec:?} {ty}");
        }
        assert!(!registry.contains(3), "{spec:?} has no EIP-4844");
        assert_eq!(registry.contains(4), spec.is_enabled_in(OpSpecId::ISTHMUS), "{spec:?}");
    }
}

#[test]
fn eip1559_priority_fee_goes_to_beneficiary() {
    let tx = TxEip1559 {
        chain_id: 10,
        gas_limit: 30_000,
        max_fee_per_gas: 10,
        max_priority_fee_per_gas: 1,
        to: TxKind::Call(TARGET),
        ..Default::default()
    };
    let tx = Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), CALLER);
    let mut evm =
        new_evm(OpSpecId::ISTHMUS, db_with_caller(300_000), block(1, 5), L1BlockInfo::default());
    let result = transact(&mut evm, &tx).unwrap();
    assert!(result.status);
    assert_eq!(balance(&mut evm, CALLER), U256::from(300_000 - 21_000 * 6));
    assert_eq!(balance(&mut evm, COINBASE), U256::from(21_000));
    assert_eq!(balance(&mut evm, BASE_FEE_RECIPIENT), U256::from(21_000 * 5));
}
