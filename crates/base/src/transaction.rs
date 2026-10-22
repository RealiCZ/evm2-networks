//! Base transaction envelope for evm2.

use alloy_consensus::{
    EthereumTxEnvelope, Signed, Transaction, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy,
    transaction::Recovered,
};
use alloy_eips::{
    eip2718::{Encodable2718, Typed2718},
    eip2930::AccessList,
    eip7702::SignedAuthorization,
};
use alloy_primitives::{B256, Bytes, ChainId, Sealed, TxKind, U256};
use base_common_consensus::{BaseTxEnvelope as BaseConsensusTxEnvelope, Eip8130Signed, TxDeposit};
use evm2::ethereum::{LazyTxEip7702, TxEnvelope};

/// The Base transaction envelope executed by [`BaseEvmTypes`](crate::BaseEvmTypes).
///
/// Covers every transaction type Base supports at the pinned revision:
///
/// - [`Deposit`](Self::Deposit): an L1-originated deposit (type `0x7e`).
/// - [`Standard`](Self::Standard): a standard Ethereum transaction (legacy, EIP-2930, EIP-1559,
///   EIP-7702), together with its EIP-2718 encoding, which prices the L1 data fee and the operator
///   fee.
/// - [`Eip8130`](Self::Eip8130): a verified EIP-8130 account-abstraction transaction (type `0x79`,
///   Zenith onwards). Unsigned RPC execution uses [`Self::Eip8130Simulation`] instead.
///
/// EIP-4844 blob transactions are not supported on Base. A [`TxEnvelope::Eip4844`] wrapped in
/// [`Standard`](Self::Standard) is rejected as an unsupported transaction type.
#[derive(Clone, Debug)]
pub enum BaseTxEnvelope {
    /// An L1-originated deposit transaction.
    Deposit(TxDeposit),
    /// A standard Ethereum transaction.
    Standard {
        /// The decoded transaction, without its signature.
        tx: TxEnvelope,
        /// The EIP-2718 encoding of the signed transaction, as posted to L1.
        ///
        /// The L1 data fee and the operator fee are priced over these bytes, matching
        /// `base_common_evm::BaseTransaction::enveloped_tx`. Empty bytes price both fees at zero,
        /// which is how `eth_call`-style simulations without a signed payload behave.
        enveloped: Bytes,
    },
    /// An enshrined EIP-8130 account-abstraction transaction.
    Eip8130(Eip8130Signed),
    /// Unsigned RPC simulation, without signature checks, nonce advancement, or fee settlement.
    Eip8130Simulation {
        /// Transaction body and representative authentication blobs for intrinsic gas pricing.
        signed: Eip8130Signed,
        /// Acting actor, or the sender's self-actor when omitted.
        sender_actor_id: Option<B256>,
    },
}

impl BaseTxEnvelope {
    /// Builds a standard-transaction envelope from a decoded transaction and its EIP-2718
    /// encoding.
    pub const fn standard(tx: TxEnvelope, enveloped: Bytes) -> Self {
        Self::Standard { tx, enveloped }
    }

    /// Builds a deposit envelope.
    pub const fn deposit(tx: TxDeposit) -> Self {
        Self::Deposit(tx)
    }

    /// Returns the deposit transaction, if this envelope is a deposit.
    pub const fn as_deposit(&self) -> Option<&TxDeposit> {
        match self {
            Self::Deposit(tx) => Some(tx),
            Self::Standard { .. } | Self::Eip8130(_) | Self::Eip8130Simulation { .. } => None,
        }
    }

    /// Returns whether this envelope is a deposit transaction.
    pub const fn is_deposit(&self) -> bool {
        matches!(self, Self::Deposit(_))
    }

    /// Returns the standard Ethereum transaction, if this envelope is one.
    pub const fn as_standard(&self) -> Option<&TxEnvelope> {
        match self {
            Self::Standard { tx, .. } => Some(tx),
            Self::Deposit(_) | Self::Eip8130(_) | Self::Eip8130Simulation { .. } => None,
        }
    }

    /// Returns the EIP-8130 transaction, if this envelope is one.
    pub const fn as_eip8130(&self) -> Option<&Eip8130Signed> {
        match self {
            Self::Eip8130(tx) | Self::Eip8130Simulation { signed: tx, .. } => Some(tx),
            Self::Deposit(_) | Self::Standard { .. } => None,
        }
    }

    /// Returns whether this envelope is an EIP-8130 transaction.
    pub const fn is_eip8130(&self) -> bool {
        matches!(self, Self::Eip8130(_) | Self::Eip8130Simulation { .. })
    }

    /// Returns the bytes the L1 data fee and the operator fee are priced over.
    ///
    /// This is the EIP-2718 encoding for standard transactions and empty for deposits, which are
    /// exempt from both fees.
    pub fn enveloped(&self) -> &[u8] {
        match self {
            Self::Standard { enveloped, .. } => enveloped,
            Self::Deposit(_) | Self::Eip8130(_) | Self::Eip8130Simulation { .. } => &[],
        }
    }

    /// Replaces the L1-fee payload of a standard transaction. A no-op for other variants.
    pub fn set_enveloped(&mut self, bytes: Bytes) {
        if let Self::Standard { enveloped, .. } = self {
            *enveloped = bytes;
        }
    }

    /// Wraps this envelope with its recovered sender, ready for [`evm2::Evm::transact`].
    ///
    /// For deposits the signer is the deposit's `from` address.
    pub const fn with_signer(self, signer: alloy_primitives::Address) -> Recovered<Self> {
        Recovered::new_unchecked(self, signer)
    }

    /// Marks an EIP-8130 envelope for unsigned RPC simulation. Other transaction types are
    /// unchanged. The optional actor hint selects the post-account-change policy context.
    pub fn set_eip8130_simulation(&mut self, sender_actor_id: Option<B256>) {
        match self {
            Self::Eip8130(signed) => {
                *self = Self::Eip8130Simulation { signed: signed.clone(), sender_actor_id };
            }
            Self::Eip8130Simulation { sender_actor_id: hint, .. } => *hint = sender_actor_id,
            _ => {}
        }
    }
}

impl From<TxDeposit> for BaseTxEnvelope {
    fn from(tx: TxDeposit) -> Self {
        Self::Deposit(tx)
    }
}

impl From<Sealed<TxDeposit>> for BaseTxEnvelope {
    fn from(tx: Sealed<TxDeposit>) -> Self {
        Self::Deposit(tx.into_inner())
    }
}

impl From<Eip8130Signed> for BaseTxEnvelope {
    fn from(tx: Eip8130Signed) -> Self {
        Self::Eip8130(tx)
    }
}

impl From<EthereumTxEnvelope<TxEip4844>> for BaseTxEnvelope {
    /// Converts a signed Ethereum transaction, encoding it to compute the L1-fee payload.
    fn from(tx: EthereumTxEnvelope<TxEip4844>) -> Self {
        let enveloped = tx.encoded_2718().into();
        Self::standard(tx.into(), enveloped)
    }
}

impl From<BaseConsensusTxEnvelope> for BaseTxEnvelope {
    /// Converts a Base consensus transaction, encoding standard transactions to compute the
    /// L1-fee payload.
    fn from(tx: BaseConsensusTxEnvelope) -> Self {
        match tx {
            BaseConsensusTxEnvelope::Legacy(tx) => tx.into(),
            BaseConsensusTxEnvelope::Eip2930(tx) => tx.into(),
            BaseConsensusTxEnvelope::Eip1559(tx) => tx.into(),
            BaseConsensusTxEnvelope::Eip7702(tx) => tx.into(),
            BaseConsensusTxEnvelope::Deposit(tx) => tx.into(),
            BaseConsensusTxEnvelope::Eip8130(tx) => tx.into(),
        }
    }
}

macro_rules! impl_from_signed {
    ($($ty:ty => $variant:ident),* $(,)?) => {
        $(
            impl From<Signed<$ty>> for BaseTxEnvelope {
                /// Converts a signed transaction, encoding it to compute the L1-fee payload.
                fn from(signed: Signed<$ty>) -> Self {
                    let enveloped = signed.encoded_2718().into();
                    Self::standard(TxEnvelope::$variant(signed.strip_signature()), enveloped)
                }
            }
        )*
    };
}

impl_from_signed!(TxLegacy => Legacy, TxEip2930 => Eip2930, TxEip1559 => Eip1559);

impl From<Signed<TxEip7702>> for BaseTxEnvelope {
    /// Converts a signed EIP-7702 transaction, encoding it to compute the L1-fee payload.
    fn from(signed: Signed<TxEip7702>) -> Self {
        let enveloped = signed.encoded_2718().into();
        let tx = LazyTxEip7702::from_recovered_authorizations(signed.strip_signature());
        Self::standard(TxEnvelope::Eip7702(tx), enveloped)
    }
}

macro_rules! delegate {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Self::Deposit(tx) => tx.$method($($arg),*),
            Self::Standard { tx, .. } => tx.$method($($arg),*),
            Self::Eip8130(tx) | Self::Eip8130Simulation { signed: tx, .. } => tx.$method($($arg),*),
        }
    };
}

impl Typed2718 for BaseTxEnvelope {
    fn ty(&self) -> u8 {
        delegate!(self, ty)
    }
}

impl Transaction for BaseTxEnvelope {
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, Signature};
    use base_common_consensus::{DEPOSIT_TX_TYPE_ID, EIP8130_TX_TYPE_ID, TxEip8130};

    fn sample_deposit() -> TxDeposit {
        TxDeposit {
            from: Address::repeat_byte(0x11),
            to: TxKind::Call(Address::repeat_byte(0x22)),
            gas_limit: 100_000,
            ..Default::default()
        }
    }

    #[test]
    fn deposit_reports_type_byte_and_is_fee_exempt() {
        let tx = BaseTxEnvelope::from(sample_deposit());
        assert_eq!(tx.ty(), DEPOSIT_TX_TYPE_ID);
        assert!(tx.is_deposit());
        assert_eq!(tx.gas_limit(), 100_000);
        assert!(tx.enveloped().is_empty());
    }

    #[test]
    fn eip8130_reports_type_byte_and_gas_limit() {
        let tx = Eip8130Signed::new(
            TxEip8130 { gas_limit: 250_000, ..Default::default() },
            Bytes::new(),
            Bytes::new(),
        );
        let envelope = BaseTxEnvelope::from(tx);
        assert_eq!(envelope.ty(), EIP8130_TX_TYPE_ID);
        assert!(envelope.is_eip8130());
        assert_eq!(envelope.gas_limit(), 250_000);
        assert!(envelope.as_standard().is_none());
    }

    #[test]
    fn consensus_conversion_keeps_the_eip2718_encoding() {
        let signed = Signed::new_unhashed(
            TxEip1559 { chain_id: 8453, gas_limit: 21_000, ..Default::default() },
            Signature::test_signature(),
        );
        let consensus = BaseConsensusTxEnvelope::Eip1559(signed);
        let expected = consensus.encoded_2718();
        let envelope = BaseTxEnvelope::from(consensus);
        assert_eq!(envelope.ty(), 2);
        assert_eq!(envelope.enveloped(), expected.as_slice());
        assert_eq!(envelope.chain_id(), Some(8453));

        let legacy = Signed::new_unhashed(TxLegacy::default(), Signature::test_signature());
        let expected = legacy.encoded_2718();
        let envelope = BaseTxEnvelope::from(legacy);
        assert_eq!(envelope.ty(), 0);
        assert_eq!(envelope.enveloped(), expected.as_slice());
    }
}
