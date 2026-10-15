//! evm2 execution type family for the OP Stack.
//!
//! A port of op-revm's execution semantics onto evm2: OP hardforks ([`OpSpecId`]), the
//! [`OpTxEnvelope`] with deposit transactions, L1 block info and the L1 data / operator fees
//! ([`L1BlockInfo`]), the fee vaults, and the per-fork precompile sets ([`OpPrecompiles`]).

pub mod constants;
pub mod precompiles;

mod evm;
mod fast_lz;
mod handler;
mod l1block;
mod spec;
mod transaction;

#[cfg(test)]
mod test_vectors;
#[cfg(test)]
mod tests;

pub use evm::{
    OpBlockEnvExt, OpConfig, OpConfigSelector, OpEvm, OpEvmExt, OpEvmFactory, OpEvmTypes,
    OpTxResultExt, op_execution_config, op_version,
};
pub use fast_lz::flz_compress_len;
pub use handler::{OpHandlerHooks, OpTxError, op_tx_registry};
pub use l1block::L1BlockInfo;
pub use precompiles::OpPrecompiles;
pub use spec::{OpSpecId, UnknownOpHardfork, name};
pub use transaction::{
    OpTxEnvelope, OpTypedTx, TxDeposit, UnsupportedEip4844, estimate_tx_compressed_size,
};
