//! OP transaction envelope.
//!
//! The envelope carries the typed transaction plus its EIP-2718 enveloped bytes, which op-revm
//! stores as `OpTransaction::enveloped_tx` and uses to compute the L1 data fee.

use crate::{constants::DEPOSIT_TRANSACTION_TYPE, fast_lz::flz_compress_len};
use alloy_consensus::{
    EthereumTxEnvelope, Signed, Transaction, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy,
};
use alloy_eips::{
    eip2718::{Encodable2718, Typed2718},
    eip2930::AccessList,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{Address, B256, Bytes, ChainId, TxKind, U256};
use evm2::ethereum::LazyTxEip7702;

/// FastLZ coefficient of the Fjord size estimate.
///
/// <https://github.com/ethereum-optimism/op-geth/blob/647c346e2bef36219cc7b47d76b1cb87e7ca29e4/core/types/rollup_cost.go#L79>
const L1_COST_FASTLZ_COEF: u64 = 836_500;

/// Intercept of the Fjord size estimate, inverted to be used with `saturating_sub`.
///
/// <https://github.com/ethereum-optimism/op-geth/blob/647c346e2bef36219cc7b47d76b1cb87e7ca29e4/core/types/rollup_cost.go#L78>
const L1_COST_INTERCEPT: u64 = 42_585_600;

/// Minimum Fjord transaction size, scaled by 1e6.
///
/// <https://github.com/ethereum-optimism/op-geth/blob/647c346e2bef36219cc7b47d76b1cb87e7ca29e4/core/types/rollup_cost.go#82>
const MIN_TX_SIZE_SCALED: u64 = 100 * 1_000_000;

/// Estimates the compressed size of a transaction, scaled by 1e6.
///
/// `max(minTransactionSize, intercept + fastlzCoef*fastlzSize)`, as in op-revm.
pub fn estimate_tx_compressed_size(input: &[u8]) -> u64 {
    let fastlz_size = flz_compress_len(input) as u64;
    fastlz_size
        .saturating_mul(L1_COST_FASTLZ_COEF)
        .saturating_sub(L1_COST_INTERCEPT)
        .max(MIN_TX_SIZE_SCALED)
}

/// OP deposit transaction (EIP-2718 type `0x7E`).
///
/// Field-for-field equivalent of `op_alloy_consensus::TxDeposit`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TxDeposit {
    /// Hash that uniquely identifies the source of the deposit.
    pub source_hash: B256,
    /// The address of the sender account.
    pub from: Address,
    /// The recipient, or [`TxKind::Create`] for a contract creation.
    pub to: TxKind,
    /// The ETH value to mint on L2.
    pub mint: u128,
    /// The ETH value to send to the recipient account.
    pub value: U256,
    /// The gas limit for the L2 transaction.
    pub gas_limit: u64,
    /// Whether this is a (pre-Regolith) system transaction.
    pub is_system_transaction: bool,
    /// Call data or init code.
    pub input: Bytes,
}

impl Typed2718 for TxDeposit {
    fn ty(&self) -> u8 {
        DEPOSIT_TRANSACTION_TYPE
    }
}

impl Transaction for TxDeposit {
    fn chain_id(&self) -> Option<ChainId> {
        None
    }

    fn nonce(&self) -> u64 {
        0
    }

    fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    fn gas_price(&self) -> Option<u128> {
        None
    }

    fn max_fee_per_gas(&self) -> u128 {
        0
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        None
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        None
    }

    fn priority_fee_or_price(&self) -> u128 {
        0
    }

    fn effective_gas_price(&self, _base_fee: Option<u64>) -> u128 {
        0
    }

    fn is_dynamic_fee(&self) -> bool {
        false
    }

    fn kind(&self) -> TxKind {
        self.to
    }

    fn is_create(&self) -> bool {
        self.to.is_create()
    }

    fn value(&self) -> U256 {
        self.value
    }

    fn input(&self) -> &Bytes {
        &self.input
    }

    fn access_list(&self) -> Option<&AccessList> {
        None
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        None
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        None
    }
}

/// Unsigned OP transaction body. There is no EIP-4844 variant on OP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpTypedTx {
    /// Legacy transaction.
    Legacy(TxLegacy),
    /// EIP-2930 access-list transaction.
    Eip2930(TxEip2930),
    /// EIP-1559 dynamic-fee transaction.
    Eip1559(TxEip1559),
    /// EIP-7702 set-code transaction.
    Eip7702(LazyTxEip7702),
    /// Deposit transaction.
    Deposit(TxDeposit),
}

impl From<TxLegacy> for OpTypedTx {
    fn from(tx: TxLegacy) -> Self {
        Self::Legacy(tx)
    }
}

impl From<TxEip2930> for OpTypedTx {
    fn from(tx: TxEip2930) -> Self {
        Self::Eip2930(tx)
    }
}

impl From<TxEip1559> for OpTypedTx {
    fn from(tx: TxEip1559) -> Self {
        Self::Eip1559(tx)
    }
}

impl From<TxEip7702> for OpTypedTx {
    fn from(tx: TxEip7702) -> Self {
        Self::Eip7702(tx.into())
    }
}

impl From<LazyTxEip7702> for OpTypedTx {
    fn from(tx: LazyTxEip7702) -> Self {
        Self::Eip7702(tx)
    }
}

impl From<TxDeposit> for OpTypedTx {
    fn from(tx: TxDeposit) -> Self {
        Self::Deposit(tx)
    }
}

macro_rules! delegate {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Self::Legacy(tx) => tx.$method($($arg),*),
            Self::Eip2930(tx) => tx.$method($($arg),*),
            Self::Eip1559(tx) => tx.$method($($arg),*),
            Self::Eip7702(tx) => tx.$method($($arg),*),
            Self::Deposit(tx) => tx.$method($($arg),*),
        }
    };
}

impl Typed2718 for OpTypedTx {
    fn ty(&self) -> u8 {
        delegate!(self, ty)
    }
}

impl Transaction for OpTypedTx {
    fn chain_id(&self) -> Option<ChainId> {
        delegate!(self, chain_id)
    }

    fn nonce(&self) -> u64 {
        delegate!(self, nonce)
    }

    fn gas_limit(&self) -> u64 {
        delegate!(self, gas_limit)
    }

    fn gas_price(&self) -> Option<u128> {
        delegate!(self, gas_price)
    }

    fn max_fee_per_gas(&self) -> u128 {
        delegate!(self, max_fee_per_gas)
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        delegate!(self, max_priority_fee_per_gas)
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        delegate!(self, max_fee_per_blob_gas)
    }

    fn priority_fee_or_price(&self) -> u128 {
        delegate!(self, priority_fee_or_price)
    }

    fn effective_gas_price(&self, base_fee: Option<u64>) -> u128 {
        delegate!(self, effective_gas_price, base_fee)
    }

    fn is_dynamic_fee(&self) -> bool {
        delegate!(self, is_dynamic_fee)
    }

    fn kind(&self) -> TxKind {
        delegate!(self, kind)
    }

    fn is_create(&self) -> bool {
        delegate!(self, is_create)
    }

    fn value(&self) -> U256 {
        delegate!(self, value)
    }

    fn input(&self) -> &Bytes {
        delegate!(self, input)
    }

    fn access_list(&self) -> Option<&AccessList> {
        delegate!(self, access_list)
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        delegate!(self, blob_versioned_hashes)
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        delegate!(self, authorization_list)
    }
}

/// OP transaction envelope used as [`EvmTypesHost::Tx`](evm2::EvmTypesHost::Tx).
///
/// Pairs the typed transaction with its EIP-2718 enveloped bytes (`enveloped_tx`), from which the
/// L1 data fee and the operator fee are computed. Non-deposit transactions without enveloped bytes
/// are rejected with [`OpTxError::MissingEnvelopedTx`](crate::OpTxError::MissingEnvelopedTx), like
/// op-revm. Pass `Some(Bytes::new())` for simulations that should not pay an L1 data fee.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpTxEnvelope {
    /// The typed transaction.
    pub tx: OpTypedTx,
    /// EIP-2718 encoding of the signed transaction, used for L1 fee computation.
    pub enveloped_tx: Option<Bytes>,
}

impl OpTxEnvelope {
    /// Creates an envelope from a typed transaction and its enveloped bytes.
    pub fn new(tx: impl Into<OpTypedTx>, enveloped_tx: Option<Bytes>) -> Self {
        Self { tx: tx.into(), enveloped_tx }
    }

    /// Creates an envelope from a signed transaction, computing its EIP-2718 encoding.
    pub fn from_signed<T>(signed: Signed<T>) -> Self
    where
        T: Into<OpTypedTx>,
        Signed<T>: Encodable2718,
    {
        let enveloped = Bytes::from(signed.encoded_2718());
        Self::new(signed.strip_signature(), Some(enveloped))
    }

    /// Creates a deposit envelope. Deposits pay no L1 fee, so no enveloped bytes are needed.
    pub fn deposit(tx: TxDeposit) -> Self {
        Self::new(tx, None)
    }

    /// Returns the typed transaction.
    pub const fn tx(&self) -> &OpTypedTx {
        &self.tx
    }

    /// Returns the EIP-2718 enveloped bytes, if set.
    pub fn enveloped_tx(&self) -> Option<&Bytes> {
        self.enveloped_tx.as_ref()
    }

    /// Returns `true` for deposit transactions.
    pub const fn is_deposit(&self) -> bool {
        matches!(self.tx, OpTypedTx::Deposit(_))
    }

    /// Returns the contained legacy transaction, if this is legacy.
    pub const fn as_legacy(&self) -> Option<&TxLegacy> {
        match &self.tx {
            OpTypedTx::Legacy(tx) => Some(tx),
            _ => None,
        }
    }

    /// Returns the contained EIP-2930 transaction, if this is EIP-2930.
    pub const fn as_eip2930(&self) -> Option<&TxEip2930> {
        match &self.tx {
            OpTypedTx::Eip2930(tx) => Some(tx),
            _ => None,
        }
    }

    /// Returns the contained EIP-1559 transaction, if this is EIP-1559.
    pub const fn as_eip1559(&self) -> Option<&TxEip1559> {
        match &self.tx {
            OpTypedTx::Eip1559(tx) => Some(tx),
            _ => None,
        }
    }

    /// Returns the contained EIP-7702 transaction, if this is EIP-7702.
    pub const fn as_eip7702(&self) -> Option<&LazyTxEip7702> {
        match &self.tx {
            OpTypedTx::Eip7702(tx) => Some(tx),
            _ => None,
        }
    }

    /// Returns the contained deposit transaction, if this is a deposit.
    pub const fn as_deposit(&self) -> Option<&TxDeposit> {
        match &self.tx {
            OpTypedTx::Deposit(tx) => Some(tx),
            _ => None,
        }
    }
}

impl From<TxDeposit> for OpTxEnvelope {
    fn from(tx: TxDeposit) -> Self {
        Self::deposit(tx)
    }
}

/// Error returned when converting an Ethereum envelope that has no OP equivalent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("EIP-4844 transactions are not supported on OP Stack chains")]
pub struct UnsupportedEip4844;

impl TryFrom<EthereumTxEnvelope<TxEip4844>> for OpTxEnvelope {
    type Error = UnsupportedEip4844;

    fn try_from(tx: EthereumTxEnvelope<TxEip4844>) -> Result<Self, Self::Error> {
        Ok(match tx {
            EthereumTxEnvelope::Legacy(tx) => Self::from_signed(tx),
            EthereumTxEnvelope::Eip2930(tx) => Self::from_signed(tx),
            EthereumTxEnvelope::Eip1559(tx) => Self::from_signed(tx),
            EthereumTxEnvelope::Eip7702(tx) => Self::from_signed(tx),
            EthereumTxEnvelope::Eip4844(_) => return Err(UnsupportedEip4844),
        })
    }
}

impl Typed2718 for OpTxEnvelope {
    fn ty(&self) -> u8 {
        self.tx.ty()
    }
}

impl Transaction for OpTxEnvelope {
    fn chain_id(&self) -> Option<ChainId> {
        self.tx.chain_id()
    }

    fn nonce(&self) -> u64 {
        self.tx.nonce()
    }

    fn gas_limit(&self) -> u64 {
        self.tx.gas_limit()
    }

    fn gas_price(&self) -> Option<u128> {
        self.tx.gas_price()
    }

    fn max_fee_per_gas(&self) -> u128 {
        self.tx.max_fee_per_gas()
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        self.tx.max_priority_fee_per_gas()
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        self.tx.max_fee_per_blob_gas()
    }

    fn priority_fee_or_price(&self) -> u128 {
        self.tx.priority_fee_or_price()
    }

    fn effective_gas_price(&self, base_fee: Option<u64>) -> u128 {
        self.tx.effective_gas_price(base_fee)
    }

    fn is_dynamic_fee(&self) -> bool {
        self.tx.is_dynamic_fee()
    }

    fn kind(&self) -> TxKind {
        self.tx.kind()
    }

    fn is_create(&self) -> bool {
        self.tx.is_create()
    }

    fn value(&self) -> U256 {
        self.tx.value()
    }

    fn input(&self) -> &Bytes {
        self.tx.input()
    }

    fn access_list(&self) -> Option<&AccessList> {
        self.tx.access_list()
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        self.tx.blob_versioned_hashes()
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        self.tx.authorization_list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Signature;

    #[test]
    fn from_signed_carries_2718_encoding() {
        let tx = TxEip1559 { chain_id: 10, gas_limit: 21_000, ..Default::default() };
        let signed = Signed::new_unhashed(tx.clone(), Signature::test_signature());
        let expected = signed.encoded_2718();
        let envelope = OpTxEnvelope::from_signed(signed);
        assert_eq!(envelope.ty(), 2);
        assert_eq!(envelope.enveloped_tx().map(|b| b.to_vec()), Some(expected));
        assert_eq!(envelope.as_eip1559(), Some(&tx));
        assert!(!envelope.is_deposit());
    }

    #[test]
    fn deposit_typed_and_fee_fields() {
        let envelope = OpTxEnvelope::deposit(TxDeposit { gas_limit: 7, ..Default::default() });
        assert_eq!(envelope.ty(), DEPOSIT_TRANSACTION_TYPE);
        assert!(envelope.is_deposit());
        assert_eq!(envelope.gas_limit(), 7);
        assert_eq!(envelope.max_fee_per_gas(), 0);
        assert_eq!(envelope.enveloped_tx(), None);
    }

    #[test]
    fn rejects_eip4844() {
        let signed = Signed::new_unhashed(TxEip4844::default(), Signature::test_signature());
        assert_eq!(
            OpTxEnvelope::try_from(EthereumTxEnvelope::<TxEip4844>::Eip4844(signed)),
            Err(UnsupportedEip4844)
        );
    }
}
