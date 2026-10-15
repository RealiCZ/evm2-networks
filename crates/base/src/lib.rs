#![doc = include_str!("../README.md")]

mod spec;
pub use spec::BaseSpecId;

mod types;
pub use types::{
    BaseBlockEnvExt, BaseConfig, BaseConfigSelector, BaseEvm, BaseEvmExt, BaseEvmTypes,
    BaseTxResultExt, base_execution_config, base_version,
};

mod transaction;
pub use transaction::BaseTxEnvelope;

mod l1block;
pub use l1block::L1BlockInfo;

mod handler;
pub use handler::BaseTxHandlerHooks;

pub mod deposit;

mod registry;
pub use registry::base_tx_registry;

mod activation;

pub mod precompiles;
pub use precompiles::{ActivationAdminConfig, BasePrecompiles};

mod factory;
pub use factory::BaseEvmFactory;

pub use base_common_consensus::{Eip8130Signed, TxDeposit};
pub use base_common_genesis::BaseUpgrade;
pub use base_common_l1_fees::L1FeeParams;
