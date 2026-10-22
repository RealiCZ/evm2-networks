//! Arbitrum transaction envelope and Nitro transaction types.

use alloy_consensus::{
    EthereumTxEnvelope, Transaction, TxEip1559, TxEip2930, TxEip4844, TxEip7702, TxLegacy,
};
use alloy_eips::{eip2718::Typed2718, eip2930::AccessList, eip7702::SignedAuthorization};
use alloy_primitives::{B256, Bytes, ChainId, TxKind, U256};
use evm2::ethereum::{LazyTxEip7702, TxEnvelope};

/// Nitro `ArbitrumDepositTx` type: an ETH deposit from L1.
pub const ARBITRUM_DEPOSIT_TX_TYPE: u8 = 0x64;
/// Nitro `ArbitrumUnsignedTx` type: an L1-to-L2 message from an EOA.
pub const ARBITRUM_UNSIGNED_TX_TYPE: u8 = 0x65;
/// Nitro `ArbitrumContractTx` type: an L1-to-L2 message from a contract.
pub const ARBITRUM_CONTRACT_TX_TYPE: u8 = 0x66;
/// Nitro `ArbitrumRetryTx` type: a retryable ticket redeem.
pub const ARBITRUM_RETRY_TX_TYPE: u8 = 0x68;
/// Nitro `ArbitrumSubmitRetryableTx` type: a retryable ticket submission.
pub const ARBITRUM_SUBMIT_RETRYABLE_TX_TYPE: u8 = 0x69;
/// Nitro `ArbitrumInternalTx` type: an ArbOS block-level update (`startBlock` at the start of
/// every block, and batch posting reports).
pub const ARBITRUM_INTERNAL_TX_TYPE: u8 = 0x6a;
/// Nitro `ArbitrumLegacyTx` type: a transaction imported from Arbitrum Classic.
pub const ARBITRUM_LEGACY_TX_TYPE: u8 = 0x78;

/// Returns whether `ty` is one of Nitro's Arbitrum-specific transaction types.
///
/// They are produced by ArbOS from L1 messages and block-level updates rather than signed by
/// users, and [`arbitrum_tx_registry`](crate::arbitrum_tx_registry) does not execute them.
pub const fn is_arbitrum_tx_type(ty: u8) -> bool {
    matches!(
        ty,
        ARBITRUM_DEPOSIT_TX_TYPE
            | ARBITRUM_UNSIGNED_TX_TYPE
            | ARBITRUM_CONTRACT_TX_TYPE
            | ARBITRUM_RETRY_TX_TYPE
            | ARBITRUM_SUBMIT_RETRYABLE_TX_TYPE
            | ARBITRUM_INTERNAL_TX_TYPE
            | ARBITRUM_LEGACY_TX_TYPE
    )
}

/// Arbitrum transaction: a signed Ethereum transaction and the L1 poster gas it is charged.
///
/// Nitro charges the cost of posting a sequenced transaction's calldata to L1 as extra gas before
/// execution, the "poster gas". It derives it from the brotli-compressed size of the signed
/// transaction and ArbOS's L1 price estimate, and reports it as the receipt's `gasUsedForL1`.
/// The poster gas is supplied by the caller: the receipt value to replay a transaction, or zero to
/// simulate one without L1 charges (as Nitro does for calls that skip L1 charging).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbTxEnvelope {
    /// The Ethereum transaction.
    pub tx: TxEnvelope,
    /// L1 poster gas charged before execution.
    pub poster_gas: u64,
}

impl ArbTxEnvelope {
    /// Creates an envelope without poster gas.
    pub const fn new(tx: TxEnvelope) -> Self {
        Self { tx, poster_gas: 0 }
    }

    /// Sets the L1 poster gas.
    pub const fn with_poster_gas(mut self, poster_gas: u64) -> Self {
        self.poster_gas = poster_gas;
        self
    }
}

impl From<TxEnvelope> for ArbTxEnvelope {
    fn from(tx: TxEnvelope) -> Self {
        Self::new(tx)
    }
}

impl From<EthereumTxEnvelope<TxEip4844>> for ArbTxEnvelope {
    fn from(tx: EthereumTxEnvelope<TxEip4844>) -> Self {
        Self::new(tx.into())
    }
}

impl From<TxLegacy> for ArbTxEnvelope {
    fn from(tx: TxLegacy) -> Self {
        Self::new(TxEnvelope::Legacy(tx))
    }
}

impl From<TxEip2930> for ArbTxEnvelope {
    fn from(tx: TxEip2930) -> Self {
        Self::new(TxEnvelope::Eip2930(tx))
    }
}

impl From<TxEip1559> for ArbTxEnvelope {
    fn from(tx: TxEip1559) -> Self {
        Self::new(TxEnvelope::Eip1559(tx))
    }
}

impl From<TxEip7702> for ArbTxEnvelope {
    fn from(tx: TxEip7702) -> Self {
        Self::new(tx.into())
    }
}

impl From<LazyTxEip7702> for ArbTxEnvelope {
    fn from(tx: LazyTxEip7702) -> Self {
        Self::new(tx.into())
    }
}

impl Typed2718 for ArbTxEnvelope {
    fn ty(&self) -> u8 {
        self.tx.ty()
    }
}

impl Transaction for ArbTxEnvelope {
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
