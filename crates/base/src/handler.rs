//! Base handler hooks for standard (non-deposit) transactions.

use alloy_consensus::Transaction;
use alloy_primitives::{Address, U256};
use base_common_consensus::Predeploys;
use evm2::{
    EvmFeatures, TxResult,
    ethereum::finalize_gas,
    handler::{GasSettlement, TxHandlerHooks},
    registry::{HandlerError, HandlerResult},
};

use crate::{BaseEvm, BaseEvmTypes, BaseTxEnvelope, L1BlockInfo};

/// Base transaction handler hooks for standard transactions.
///
/// Layered on evm2's stock Ethereum handlers, these port the standard-transaction parts of
/// `base_common_evm::BaseHandler`:
///
/// - [`before_execution`](TxHandlerHooks::before_execution) loads the L1 block info for the block
///   (`validate_against_state_and_deduct_caller`), then charges the L1 data fee and the Isthmus
///   operator fee (on the gas limit) on top of the upfront gas cost. Both are skipped when
///   [`EvmFeatures::FEE_CHARGE`] is disabled.
/// - [`settle_transaction`](TxHandlerHooks::settle_transaction) refunds unused gas and the unused
///   operator fee (`reimburse_caller`), pays the coinbase the priority fee, and credits the L1 fee,
///   base fee, and operator fee vaults (`reward_beneficiary`).
#[derive(Clone, Copy, Debug, Default)]
pub struct BaseTxHandlerHooks;

impl BaseTxHandlerHooks {
    /// Ensures the cached L1 block info is for the block being executed, reloading it otherwise.
    ///
    /// A [`BaseBlockEnvExt::l1_fee_params`](crate::BaseBlockEnvExt::l1_fee_params) override wins
    /// over the `L1Block` predeploy storage.
    pub fn load_l1_block_info(host: &mut BaseEvm<'_>) -> HandlerResult<()> {
        let number = host.block().number;
        if let Some(params) = host.block().ext.l1_fee_params {
            host.ext_mut().l1_block_info = L1BlockInfo::from_params(params, Some(number));
            return Ok(());
        }
        if host.ext().l1_block_info.l2_block != Some(number) {
            let spec = host.config_spec_id();
            host.ext_mut().l1_block_info = L1BlockInfo::try_fetch(host, number, spec)?;
        }
        Ok(())
    }

    /// Returns the operator fee for `envelope` over `gas`.
    fn operator_fee(host: &BaseEvm<'_>, envelope: &BaseTxEnvelope, gas: u64) -> U256 {
        let spec = host.config_spec_id();
        host.ext().l1_block_info.operator_fee_charge(envelope.enveloped(), U256::from(gas), spec)
    }

    /// Credits `amount` to `recipient`.
    fn credit(host: &mut BaseEvm<'_>, recipient: Address, amount: U256) -> HandlerResult<()> {
        host.state_mut().account(&recipient)?.add_balance(amount);
        Ok(())
    }
}

impl TxHandlerHooks<BaseEvmTypes> for BaseTxHandlerHooks {
    fn before_execution(
        host: &mut BaseEvm<'_>,
        envelope: &BaseTxEnvelope,
        caller: Address,
        upfront_fee: U256,
    ) -> HandlerResult<()> {
        Self::load_l1_block_info(host)?;
        let ext = host.ext_mut();
        ext.tx_origin = caller;
        ext.clear_tx_l1_cost();
        ext.operator_fee_charged = U256::ZERO;

        if !host.feature(EvmFeatures::FEE_CHARGE) {
            return Ok(());
        }

        let spec = host.config_spec_id();
        let l1_fee = host.ext_mut().l1_block_info.calculate_tx_l1_cost(envelope.enveloped(), spec);
        let operator_fee = Self::operator_fee(host, envelope, envelope.gas_limit());
        host.ext_mut().operator_fee_charged = operator_fee;
        let additional = l1_fee.saturating_add(operator_fee);

        let balance_check = host.feature(EvmFeatures::BALANCE_CHECK);
        let balance_top_up = host.feature(EvmFeatures::BALANCE_TOP_UP);
        let max_upfront = U256::from(envelope.gas_limit())
            .saturating_mul(U256::from(envelope.max_fee_per_gas()))
            .saturating_add(envelope.value());
        let mut account = host.state_mut().account(&caller)?;
        let mut balance = account.balance();
        if balance_check {
            // The reference subtracts the L1 and operator fees first, then requires the remainder
            // to cover the maximum gas cost and the value (`LackOfFundForMaxFee`).
            let fee = additional.saturating_add(max_upfront);
            if balance < fee {
                return Err(HandlerError::InsufficientFunds { fee, balance });
            }
        } else if balance_top_up {
            // Extend evm2's balance top-up (see `validate_sender`) to the Base fees, so a sender
            // with balance checks disabled can always pay them.
            let required = additional.saturating_add(upfront_fee).saturating_add(envelope.value());
            if balance < required {
                balance = required;
            }
        }
        account.set_balance(balance.saturating_sub(additional).saturating_sub(upfront_fee));
        Ok(())
    }

    fn settle_transaction(
        host: &mut BaseEvm<'_>,
        envelope: &BaseTxEnvelope,
        gas: GasSettlement<BaseEvmTypes>,
    ) -> HandlerResult<TxResult<BaseEvmTypes>> {
        let caller = gas.caller;
        let gas_price = gas.gas_price;
        let gas_limit = gas.gas_limit;
        let mut result = finalize_gas(host, gas)?;
        let gas_used = result.tx_gas_used();
        let basefee = host.block().basefee;
        let spec = host.config_spec_id();
        let operator_fee = Self::operator_fee(host, envelope, gas_used);

        if host.feature(EvmFeatures::FEE_CHARGE) {
            // `reimburse_caller`: unused gas at the effective price, plus the operator fee charged
            // on the unused gas.
            let gas_refund = U256::from(gas_limit.saturating_sub(gas_used)) * gas_price;
            let operator_fee_refund = host.ext().operator_fee_charged.saturating_sub(operator_fee);
            Self::credit(host, caller, gas_refund.saturating_add(operator_fee_refund))?;

            // `reward_beneficiary`: the coinbase earns the priority fee. Base is always
            // post-London, so the base fee is never paid to the coinbase.
            let beneficiary = host.block().beneficiary;
            let reward = U256::from(gas_used) * gas_price.saturating_sub(basefee);
            Self::credit(host, beneficiary, reward)?;
        }

        // The reference credits the fee vaults regardless of whether fee charging is enabled.
        let l1_fee = host.ext_mut().l1_block_info.calculate_tx_l1_cost(envelope.enveloped(), spec);
        Self::credit(host, Predeploys::L1_FEE_VAULT, l1_fee)?;
        Self::credit(
            host,
            Predeploys::BASE_FEE_VAULT,
            basefee.saturating_mul(U256::from(gas_used)),
        )?;
        Self::credit(host, Predeploys::OPERATOR_FEE_VAULT, operator_fee)?;

        let ext = host.ext_mut();
        ext.clear_tx_l1_cost();
        ext.operator_fee_charged = U256::ZERO;

        result.ext.l1_fee = l1_fee;
        result.ext.operator_fee = operator_fee;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseBlockEnvExt, BaseEvmFactory, BaseSpecId, BaseUpgrade, L1FeeParams};
    use alloy_consensus::TxLegacy;
    use alloy_primitives::{Bytes, TxKind};
    use evm2::{env::BlockEnv, ethereum::TxEnvelope, evm::InMemoryDB};

    const SENDER: Address = Address::repeat_byte(0x11);

    fn evm(block: BlockEnv<BaseEvmTypes>) -> BaseEvm<'static> {
        let spec = BaseSpecId::new(BaseUpgrade::Regolith);
        let factory = BaseEvmFactory::default();
        let mut version = factory.version(spec, 8453);
        version.features.remove(EvmFeatures::BALANCE_CHECK);
        factory.create_evm(spec, version, block, InMemoryDB::default())
    }

    fn l1_fee(evm: &mut BaseEvm<'_>, nonce: u64) -> U256 {
        let tx = TxLegacy {
            nonce,
            gas_limit: 21_000,
            to: TxKind::Call(Address::ZERO),
            ..Default::default()
        };
        // Three non-zero bytes priced at Regolith: (3 * 16 + 1000) * 1000 * 1000 / 1e6 = 1048.
        let tx = BaseTxEnvelope::standard(TxEnvelope::Legacy(tx), Bytes::from_static(&[1, 2, 3]));
        evm.transact(&tx.with_signer(SENDER)).unwrap().commit().ext.l1_fee
    }

    fn params() -> L1FeeParams {
        L1FeeParams {
            l1_base_fee: U256::from(1_000),
            l1_fee_overhead: Some(U256::from(1_000)),
            l1_base_fee_scalar: U256::from(1_000),
            ..Default::default()
        }
    }

    #[test]
    fn l1_block_info_is_cached_per_block_and_reloaded_on_a_new_block() {
        let block = BlockEnv::<BaseEvmTypes> { number: U256::from(5), ..Default::default() };
        let mut evm = evm(block);
        // Info preloaded for the current block is used as-is, like the reference's chain context.
        evm.ext_mut().l1_block_info = L1BlockInfo::from_params(params(), Some(U256::from(5)));
        assert_eq!(l1_fee(&mut evm, 0), U256::from(1048));
        assert_eq!(l1_fee(&mut evm, 1), U256::from(1048));
        // A new block reloads the info from the (empty) `L1Block` predeploy storage.
        evm.set_block(BlockEnv::<BaseEvmTypes> { number: U256::from(6), ..Default::default() });
        assert_eq!(l1_fee(&mut evm, 2), U256::ZERO);
        assert_eq!(evm.ext().l1_block_info.l2_block, Some(U256::from(6)));
    }

    #[test]
    fn block_env_override_wins_over_storage() {
        let block = BlockEnv::<BaseEvmTypes> {
            number: U256::from(5),
            ext: BaseBlockEnvExt { l1_fee_params: Some(params()) },
            ..Default::default()
        };
        let mut evm = evm(block);
        assert_eq!(l1_fee(&mut evm, 0), U256::from(1048));
    }
}
