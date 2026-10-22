//! Arbitrum precompiles.
//!
//! Port of Nitro's precompile sets (`gethhook/geth-hook.go`) and of part of its ArbOS
//! precompiles (`precompiles/`):
//!
//! | ArbOS version | Ethereum precompiles                                          |
//! |---------------|---------------------------------------------------------------|
//! | Before 30     | Berlin set                                                    |
//! | 30 to 49      | Cancun set (with KZG) plus RIP-7212 P256 (`0x100`, 3,450 gas) |
//! | From 50       | Osaka set (BLS12-381, Osaka modexp, P256 at 6,900 gas)        |
//!
//! The ArbOS precompiles live at `0x64`-`0x74`, `0xff` and `0xa4b05`. Each is installed from the
//! ArbOS version that introduced it, so it is warm and its address has no code, as on Nitro.
//! [`ArbSys`](ARB_SYS_ADDRESS), [`ArbInfo`](ARB_INFO_ADDRESS) and the getters of
//! [`ArbGasInfo`](ARB_GAS_INFO_ADDRESS) are ported, and `ArbDebug` fails as on chains without debug
//! mode. Calling any other ArbOS precompile or method aborts the transaction with a fatal
//! [`ArbOsPrecompileUnsupported`] error rather than diverging silently.
//!
//! The ported methods follow Nitro's precompile framework (`precompiles/precompile.go`):
//!
//! - A call with an unknown or inactive selector, malformed arguments, value sent to a non-payable
//!   method, or a `view` method called through `DELEGATECALL` or `CALLCODE` reverts and consumes
//!   all gas.
//! - Methods that read the ArbOS state pay 800 gas to open it, and 800 gas per storage read.
//! - Arguments and results cost 3 gas per 32-byte word.

use crate::{
    ARBOS_ADDRESS, ArbEvm, ArbEvmTypes, ArbOsVersion,
    state::{ArbOsStorage, l1_pricing, l1_pricing_storage, l2_pricing, l2_pricing_storage},
};
use alloc::{borrow::Cow, vec::Vec};
use alloy_primitives::{Address, B256, Bytes, I256, U256, address, uint};
use alloy_sol_types::{SolInterface, sol};
use evm2::{
    Precompiles, SpecId,
    evm::precompile::PrecompileOutput,
    interpreter::{GasTracker, Host, Message, MessageKind},
    precompiles::{
        P256VERIFY, Precompile, PrecompileError, PrecompileHalt, PrecompileId, PrecompileMap,
        PrecompileResult,
    },
};

/// `ArbSys` precompile address: L2 block and chain information.
pub const ARB_SYS_ADDRESS: Address = address!("0x0000000000000000000000000000000000000064");
/// `ArbInfo` precompile address: account balance and code lookups.
pub const ARB_INFO_ADDRESS: Address = address!("0x0000000000000000000000000000000000000065");
/// `ArbGasInfo` precompile address: gas pricing information.
pub const ARB_GAS_INFO_ADDRESS: Address = address!("0x000000000000000000000000000000000000006c");
/// `ArbDebug` precompile address, only enabled on debug chains.
pub const ARB_DEBUG_ADDRESS: Address = address!("0x00000000000000000000000000000000000000ff");

/// Error aborting a transaction that calls an ArbOS precompile method this crate does not port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArbOsPrecompileUnsupported {
    /// Precompile name.
    pub precompile: &'static str,
    /// Method name, or `None` when the whole precompile is unsupported.
    pub method: Option<&'static str>,
}

impl core::fmt::Display for ArbOsPrecompileUnsupported {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.method {
            Some(method) => {
                write!(f, "ArbOS precompile method {}.{method} is not supported", self.precompile)
            }
            None => write!(f, "ArbOS precompile {} is not supported", self.precompile),
        }
    }
}

impl core::error::Error for ArbOsPrecompileUnsupported {}

sol! {
    /// Ported and known methods of `ArbSys`.
    #[allow(missing_docs)]
    interface IArbSys {
        function arbBlockNumber() external view returns (uint256);
        function arbBlockHash(uint256 arbBlockNum) external view returns (bytes32);
        function arbChainID() external view returns (uint256);
        function arbOSVersion() external view returns (uint256);
        function getStorageGasAvailable() external view returns (uint256);
        function isTopLevelCall() external view returns (bool);
        function mapL1SenderContractAddressToL2Alias(address sender, address unused) external pure returns (address);
        function wasMyCallersAddressAliased() external view returns (bool);
        function myCallersAddressWithoutAliasing() external view returns (address);
        function withdrawEth(address destination) external payable returns (uint256);
        function sendTxToL1(address destination, bytes calldata data) external payable returns (uint256);
        function sendMerkleTreeState() external view returns (uint256, bytes32, bytes32[] memory);

        error InvalidBlockNumber(uint256 requested, uint256 current);
    }

    /// Methods of `ArbInfo`.
    #[allow(missing_docs)]
    interface IArbInfo {
        function getBalance(address account) external view returns (uint256);
        function getCode(address account) external view returns (bytes memory);
    }

    /// Methods of `ArbGasInfo`.
    #[allow(missing_docs)]
    interface IArbGasInfo {
        function getPricesInWeiWithAggregator(address aggregator) external view returns (uint256, uint256, uint256, uint256, uint256, uint256);
        function getPricesInWei() external view returns (uint256, uint256, uint256, uint256, uint256, uint256);
        function getPricesInArbGasWithAggregator(address aggregator) external view returns (uint256, uint256, uint256);
        function getPricesInArbGas() external view returns (uint256, uint256, uint256);
        function getGasAccountingParams() external view returns (uint256, uint256, uint256);
        function getMaxTxGasLimit() external view returns (uint256);
        function getMinimumGasPrice() external view returns (uint256);
        function getL1BaseFeeEstimate() external view returns (uint256);
        function getL1BaseFeeEstimateInertia() external view returns (uint64);
        function getL1RewardRate() external view returns (uint64);
        function getL1RewardRecipient() external view returns (address);
        function getL1GasPriceEstimate() external view returns (uint256);
        function getCurrentTxL1GasFees() external view returns (uint256);
        function getGasBacklog() external view returns (uint64);
        function getPricingInertia() external view returns (uint64);
        function getGasBacklogTolerance() external view returns (uint64);
        function getL1PricingSurplus() external view returns (int256);
        function getPerBatchGasCharge() external view returns (int64);
        function getAmortizedCostCapBips() external view returns (uint64);
        function getL1FeesAvailable() external view returns (uint256);
        function getL1PricingEquilibrationUnits() external view returns (uint256);
        function getLastL1PricingUpdateTime() external view returns (uint64);
        function getL1PricingFundsDueForRewards() external view returns (uint256);
        function getL1PricingUnitsSinceUpdate() external view returns (uint64);
        function getLastL1PricingSurplus() external view returns (int256);
        function getMaxBlockGasLimit() external view returns (uint64);
        function getGasPricingConstraints() external view returns (uint64[3][] memory);
        function getMultiGasPricingConstraints() external view;
        function getMultiGasBaseFee() external view returns (uint256[] memory);
    }
}

/// State mutability of an ArbOS precompile method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Purity {
    Pure,
    View,
    Payable,
}

/// Offset added to L1 contract addresses to alias them on L2.
const ADDRESS_ALIAS_OFFSET: U256 = uint!(0x1111000000000000000000000000000000001111_U256);

/// Gas per storage read (`storage.StorageReadCost`).
const STORAGE_READ_GAS: u64 = 800;
/// Gas per 32-byte word of arguments and results (`params.CopyGas`).
const COPY_GAS: u64 = 3;
/// Gas cost of an L2 storage write assumed by `ArbGasInfo` (`storage.StorageWriteCost`).
const STORAGE_WRITE_GAS: u64 = 20_000;
/// Transaction size assumed by `ArbGasInfo` price estimates (`AssumedSimpleTxSize`).
const ASSUMED_SIMPLE_TX_SIZE: u64 = 140;
/// Gas per non-zero calldata byte (EIP-2028).
const TX_DATA_NON_ZERO_GAS: u64 = 16;

/// A call to an ArbOS precompile, after the framework checks.
struct ArbOsCall<'e, 'a> {
    evm: &'e mut ArbEvm<'a>,
    message: &'e Message<ArbEvmTypes>,
    gas: &'e mut GasTracker,
    version: ArbOsVersion,
}

impl ArbOsCall<'_, '_> {
    fn burn(&mut self, amount: u64) -> Result<(), PrecompileError> {
        self.gas.spend(amount).map_err(|_| PrecompileHalt::OutOfGas.into())
    }

    fn read(&mut self, storage: ArbOsStorage, offset: u64) -> Result<U256, PrecompileError> {
        self.burn(STORAGE_READ_GAS)?;
        Ok(storage.get(self.evm.state_mut(), offset)?)
    }

    fn read_u64(&mut self, storage: ArbOsStorage, offset: u64) -> Result<u64, PrecompileError> {
        self.read(storage, offset).map(|value| value.as_limbs()[0])
    }

    /// Returns the call depth relative to the transaction's top-level frame (Nitro's
    /// `evm.Depth() - 1` inside a precompile).
    fn depth(&self) -> u16 {
        self.message.depth.saturating_sub(self.evm.version().initial_call_depth)
    }

    fn basefee(&self) -> U256 {
        self.evm.block().basefee
    }
}

/// Outcome of the framework checks preceding a method.
fn check_call(
    message: &Message<ArbEvmTypes>,
    gas: &mut GasTracker,
    address: Address,
    purity: Purity,
    active: bool,
) -> Result<(), PrecompileError> {
    let acting_as_precompile = message.destination == address;
    if !active
        || (purity >= Purity::View && !acting_as_precompile)
        || (purity < Purity::Payable && !message.value.is_zero())
    {
        return Err(revert_all(gas));
    }
    Ok(())
}

/// Reverts consuming all gas, as Nitro does for calls its framework rejects.
fn revert_all(gas: &mut GasTracker) -> PrecompileError {
    gas.spend_all();
    PrecompileError::Revert(Bytes::new())
}

/// Charges the framework gas paid before a method runs: opening the ArbOS state for non-pure
/// methods, then the argument words.
fn charge_entry(gas: &mut GasTracker, input: &[u8], purity: Purity) -> Result<(), PrecompileError> {
    if purity != Purity::Pure {
        gas.spend(STORAGE_READ_GAS).map_err(|_| PrecompileHalt::OutOfGas)?;
    }
    let args_words = (input.len().saturating_sub(4) as u64).div_ceil(32);
    if gas.spend(COPY_GAS * args_words).is_err() {
        return Err(revert_all(gas));
    }
    Ok(())
}

/// Charges the result words and returns the output.
fn finish(gas: &mut GasTracker, output: Vec<u8>) -> PrecompileResult {
    let words = (output.len() as u64).div_ceil(32);
    if gas.spend(COPY_GAS * words).is_err() {
        return Err(revert_all(gas));
    }
    Ok(PrecompileOutput::new(output.into()))
}

/// ABI-encodes static words.
fn encode_words(words: &[U256]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_be_bytes::<32>()).collect()
}

fn unsupported(precompile: &'static str, method: &'static str) -> PrecompileError {
    PrecompileError::fatal(ArbOsPrecompileUnsupported { precompile, method: Some(method) })
}

/// Executes `ArbSys` (port of `precompiles/ArbSys.go`).
fn arb_sys(
    evm: &mut ArbEvm<'_>,
    message: &Message<ArbEvmTypes>,
    gas: &mut GasTracker,
) -> PrecompileResult {
    use IArbSys::IArbSysCalls as C;
    let Ok(call) = C::abi_decode(&message.input) else { return Err(revert_all(gas)) };
    let purity = match call {
        C::mapL1SenderContractAddressToL2Alias(_) => Purity::Pure,
        C::withdrawEth(_) | C::sendTxToL1(_) => Purity::Payable,
        _ => Purity::View,
    };
    check_call(message, gas, ARB_SYS_ADDRESS, purity, true)?;
    if matches!(call, C::withdrawEth(_) | C::sendTxToL1(_))
        && (message.caller_is_static || message.kind == MessageKind::StaticCall)
    {
        return Err(revert_all(gas));
    }
    charge_entry(gas, &message.input, purity)?;
    let version = evm.config_spec_id();
    let mut cx = ArbOsCall { evm, message, gas, version };
    let output = match call {
        C::arbBlockNumber(_) => encode_words(&[cx.evm.block().number]),
        C::arbBlockHash(IArbSys::arbBlockHashCall { arbBlockNum: requested }) => {
            let current = cx.evm.block().number;
            let in_range =
                requested < current && requested.saturating_add(U256::from(256)) >= current;
            if !in_range {
                if !cx.version.enables(ArbOsVersion::V11) {
                    return Err(PrecompileHalt::Other(Cow::Borrowed("invalid block number")).into());
                }
                let data = alloy_sol_types::SolError::abi_encode(&IArbSys::InvalidBlockNumber {
                    requested,
                    current,
                });
                let words = (data.len() as u64).div_ceil(32);
                cx.burn(COPY_GAS * words)?;
                return Err(PrecompileError::Revert(data.into()));
            }
            let hash = Host::block_hash(cx.evm, &requested)?;
            hash.to_vec()
        }
        C::arbChainID(_) => encode_words(&[U256::from(cx.evm.version().chain_id)]),
        C::arbOSVersion(_) => encode_words(&[U256::from(55 + cx.version.get())]),
        C::getStorageGasAvailable(_) => encode_words(&[U256::ZERO]),
        C::isTopLevelCall(_) => encode_words(&[U256::from(cx.depth() <= 1)]),
        C::mapL1SenderContractAddressToL2Alias(call) => {
            let aliased =
                U256::from_be_slice(call.sender.as_slice()).wrapping_add(ADDRESS_ALIAS_OFFSET);
            let aliased = Address::from_word(B256::from(aliased));
            encode_words(&[U256::from_be_slice(aliased.as_slice())])
        }
        // Only Nitro's L1-to-L2 transaction types alias their sender.
        C::wasMyCallersAddressAliased(_) => encode_words(&[U256::ZERO]),
        C::myCallersAddressWithoutAliasing(_) => {
            // The caller of the frame calling the precompile: none for a transaction calling
            // `ArbSys` directly, the sender for the top-level frame.
            let address = match cx.depth() {
                0 => Address::ZERO,
                1 => cx.evm.ext().tx.sender,
                _ => return Err(unsupported("ArbSys", "myCallersAddressWithoutAliasing")),
            };
            encode_words(&[U256::from_be_slice(address.as_slice())])
        }
        C::withdrawEth(_) => return Err(unsupported("ArbSys", "withdrawEth")),
        C::sendTxToL1(_) => return Err(unsupported("ArbSys", "sendTxToL1")),
        C::sendMerkleTreeState(_) => return Err(unsupported("ArbSys", "sendMerkleTreeState")),
    };
    finish(cx.gas, output)
}

/// Executes `ArbInfo` (port of `precompiles/ArbInfo.go`).
fn arb_info(
    evm: &mut ArbEvm<'_>,
    message: &Message<ArbEvmTypes>,
    gas: &mut GasTracker,
) -> PrecompileResult {
    use IArbInfo::IArbInfoCalls as C;
    let Ok(call) = C::abi_decode(&message.input) else { return Err(revert_all(gas)) };
    check_call(message, gas, ARB_INFO_ADDRESS, Purity::View, true)?;
    charge_entry(gas, &message.input, Purity::View)?;
    let version = evm.config_spec_id();
    let mut cx = ArbOsCall { evm, message, gas, version };
    let output = match call {
        C::getBalance(call) => {
            cx.burn(700)?;
            let balance = cx.evm.state_mut().account(&call.account)?.balance();
            encode_words(&[balance])
        }
        C::getCode(call) => {
            cx.burn(2_100)?;
            let code = cx.evm.state_mut().account(&call.account)?.load_code()?;
            let code = code.original_bytes();
            cx.burn(COPY_GAS * (code.len() as u64).div_ceil(32))?;
            alloy_sol_types::SolValue::abi_encode(&code)
        }
    };
    finish(cx.gas, output)
}

/// Executes `ArbGasInfo` (port of the getters of `precompiles/ArbGasInfo.go`).
fn arb_gas_info(
    evm: &mut ArbEvm<'_>,
    message: &Message<ArbEvmTypes>,
    gas: &mut GasTracker,
) -> PrecompileResult {
    use IArbGasInfo::IArbGasInfoCalls as C;
    let Ok(call) = C::abi_decode(&message.input) else { return Err(revert_all(gas)) };
    let version = evm.config_spec_id();
    let since = match call {
        C::getL1FeesAvailable(_) => ArbOsVersion::V10,
        C::getL1RewardRate(_) | C::getL1RewardRecipient(_) => ArbOsVersion::V11,
        C::getL1PricingEquilibrationUnits(_)
        | C::getLastL1PricingUpdateTime(_)
        | C::getL1PricingFundsDueForRewards(_)
        | C::getL1PricingUnitsSinceUpdate(_)
        | C::getLastL1PricingSurplus(_) => ArbOsVersion::V20,
        C::getMaxTxGasLimit(_) | C::getMaxBlockGasLimit(_) | C::getGasPricingConstraints(_) => {
            ArbOsVersion::V50
        }
        C::getMultiGasPricingConstraints(_) | C::getMultiGasBaseFee(_) => ArbOsVersion::V60,
        _ => ArbOsVersion::new(0),
    };
    check_call(message, gas, ARB_GAS_INFO_ADDRESS, Purity::View, version.enables(since))?;
    charge_entry(gas, &message.input, Purity::View)?;
    let mut cx = ArbOsCall { evm, message, gas, version };
    let l1 = l1_pricing_storage();
    let l2 = l2_pricing_storage();
    let output = match call {
        C::getPricesInWeiWithAggregator(_) | C::getPricesInWei(_) => {
            let l1_gas_price = cx.read(l1, l1_pricing::PRICE_PER_UNIT)?;
            let l2_gas_price = cx.basefee();
            let wei_for_l1_calldata = l1_gas_price * U256::from(TX_DATA_NON_ZERO_GAS);
            let per_l2_tx = wei_for_l1_calldata * U256::from(ASSUMED_SIMPLE_TX_SIZE);
            let wei_for_l2_storage = l2_gas_price * U256::from(STORAGE_WRITE_GAS);
            let (base, congestion) = if cx.version.enables(ArbOsVersion::V4) {
                let base = cx.read(l2, l2_pricing::MIN_BASE_FEE_WEI)?.min(l2_gas_price);
                (base, l2_gas_price - base)
            } else {
                (l2_gas_price, U256::ZERO)
            };
            encode_words(&[
                per_l2_tx,
                wei_for_l1_calldata,
                wei_for_l2_storage,
                base,
                congestion,
                l2_gas_price,
            ])
        }
        C::getPricesInArbGasWithAggregator(_) | C::getPricesInArbGas(_) => {
            let l1_gas_price = cx.read(l1, l1_pricing::PRICE_PER_UNIT)?;
            let l2_gas_price = cx.basefee();
            let wei_for_l1_calldata = l1_gas_price * U256::from(TX_DATA_NON_ZERO_GAS);
            let gas_for_l1_calldata = wei_for_l1_calldata.checked_div(l2_gas_price);
            let gas_per_l2_tx = if cx.version.enables(ArbOsVersion::V4) {
                (wei_for_l1_calldata * U256::from(ASSUMED_SIMPLE_TX_SIZE))
                    .checked_div(l2_gas_price)
                    .unwrap_or_default()
            } else {
                U256::from(ASSUMED_SIMPLE_TX_SIZE)
            };
            encode_words(&[
                gas_per_l2_tx,
                gas_for_l1_calldata.unwrap_or_default(),
                U256::from(STORAGE_WRITE_GAS),
            ])
        }
        C::getGasAccountingParams(_) => {
            let speed_limit = cx.read_u64(l2, l2_pricing::SPEED_LIMIT_PER_SECOND)?;
            let block_gas_limit = cx.read_u64(l2, l2_pricing::PER_BLOCK_GAS_LIMIT)?;
            encode_words(&[
                U256::from(speed_limit),
                U256::from(block_gas_limit),
                U256::from(block_gas_limit),
            ])
        }
        C::getMaxTxGasLimit(_) => {
            encode_words(&[U256::from(cx.read_u64(l2, l2_pricing::PER_TX_GAS_LIMIT)?)])
        }
        C::getMinimumGasPrice(_) => encode_words(&[cx.read(l2, l2_pricing::MIN_BASE_FEE_WEI)?]),
        C::getL1BaseFeeEstimate(_) | C::getL1GasPriceEstimate(_) => {
            encode_words(&[cx.read(l1, l1_pricing::PRICE_PER_UNIT)?])
        }
        C::getL1BaseFeeEstimateInertia(_) => {
            encode_words(&[U256::from(cx.read_u64(l1, l1_pricing::INERTIA)?)])
        }
        C::getL1RewardRate(_) => {
            encode_words(&[U256::from(cx.read_u64(l1, l1_pricing::PER_UNIT_REWARD)?)])
        }
        C::getL1RewardRecipient(_) => {
            let recipient = cx.read(l1, l1_pricing::PAY_REWARDS_TO)?;
            encode_words(&[U256::from_be_slice(&recipient.to_be_bytes::<32>()[12..])])
        }
        C::getCurrentTxL1GasFees(_) => encode_words(&[cx.evm.ext().tx.poster_fee]),
        C::getGasBacklog(_) => {
            encode_words(&[U256::from(cx.read_u64(l2, l2_pricing::GAS_BACKLOG)?)])
        }
        C::getPricingInertia(_) => {
            encode_words(&[U256::from(cx.read_u64(l2, l2_pricing::PRICING_INERTIA)?)])
        }
        C::getGasBacklogTolerance(_) => {
            encode_words(&[U256::from(cx.read_u64(l2, l2_pricing::BACKLOG_TOLERANCE)?)])
        }
        C::getPerBatchGasCharge(_) => {
            let cost = cx.read_u64(l1, l1_pricing::PER_BATCH_GAS_COST)? as i64;
            encode_words(&[I256::try_from(cost).expect("i64 fits in I256").into_raw()])
        }
        C::getAmortizedCostCapBips(_) => {
            encode_words(&[U256::from(cx.read_u64(l1, l1_pricing::AMORTIZED_COST_CAP_BIPS)?)])
        }
        C::getL1FeesAvailable(_) => encode_words(&[cx.read(l1, l1_pricing::L1_FEES_AVAILABLE)?]),
        C::getL1PricingEquilibrationUnits(_) => {
            encode_words(&[cx.read(l1, l1_pricing::EQUILIBRATION_UNITS)?])
        }
        C::getLastL1PricingUpdateTime(_) => {
            encode_words(&[U256::from(cx.read_u64(l1, l1_pricing::LAST_UPDATE_TIME)?)])
        }
        // Signed values are stored in two's complement, which is also their ABI encoding.
        C::getL1PricingFundsDueForRewards(_) => {
            encode_words(&[cx.read(l1, l1_pricing::FUNDS_DUE_FOR_REWARDS)?])
        }
        C::getL1PricingUnitsSinceUpdate(_) => {
            encode_words(&[U256::from(cx.read_u64(l1, l1_pricing::UNITS_SINCE_UPDATE)?)])
        }
        C::getLastL1PricingSurplus(_) => encode_words(&[cx.read(l1, l1_pricing::LAST_SURPLUS)?]),
        C::getMaxBlockGasLimit(_) => {
            encode_words(&[U256::from(cx.read_u64(l2, l2_pricing::PER_BLOCK_GAS_LIMIT)?)])
        }
        C::getGasPricingConstraints(_) => {
            let constraints = l2.open(&[l2_pricing::GAS_CONSTRAINTS]);
            let len = cx.read_u64(constraints, 0)?;
            let mut words = Vec::with_capacity(2 + 3 * len as usize);
            words.extend([U256::from(32), U256::from(len)]);
            for index in 0..len {
                let constraint = crate::state::vector_element(&constraints, index);
                for offset in 0..3 {
                    words.push(U256::from(cx.read_u64(constraint, offset)?));
                }
            }
            encode_words(&words)
        }
        C::getL1PricingSurplus(_) => return Err(unsupported("ArbGasInfo", "getL1PricingSurplus")),
        C::getMultiGasPricingConstraints(_) => {
            return Err(unsupported("ArbGasInfo", "getMultiGasPricingConstraints"));
        }
        C::getMultiGasBaseFee(_) => return Err(unsupported("ArbGasInfo", "getMultiGasBaseFee")),
    };
    finish(cx.gas, output)
}

/// Executes `ArbDebug`, which takes all gas and fails on chains without debug mode.
fn arb_debug(
    _evm: &mut ArbEvm<'_>,
    _message: &Message<ArbEvmTypes>,
    _gas: &mut GasTracker,
) -> PrecompileResult {
    Err(PrecompileHalt::Other(Cow::Borrowed("debug precompiles are disabled")).into())
}

macro_rules! unsupported_precompiles {
    ($($name:literal, $address:literal, $since:expr => $entry:ident;)*) => {
        $(
            fn $entry(
                _evm: &mut ArbEvm<'_>,
                _message: &Message<ArbEvmTypes>,
                _gas: &mut GasTracker,
            ) -> PrecompileResult {
                Err(PrecompileError::fatal(ArbOsPrecompileUnsupported {
                    precompile: $name,
                    method: None,
                }))
            }
        )*

        /// ArbOS precompiles that are not ported, with the ArbOS version introducing them.
        fn unsupported_precompiles() -> impl Iterator<Item = (ArbOsVersion, Precompile<ArbEvmTypes>)> {
            [$((
                $since,
                Precompile::new(
                    Address::with_last_byte($address),
                    PrecompileId::custom($name),
                    $entry,
                ),
            ),)*]
            .into_iter()
        }
    };
}

unsupported_precompiles! {
    "ArbAddressTable", 0x66, ArbOsVersion::new(0) => arb_address_table;
    "ArbBLS", 0x67, ArbOsVersion::new(0) => arb_bls;
    "ArbFunctionTable", 0x68, ArbOsVersion::new(0) => arb_function_table;
    "ArbosTest", 0x69, ArbOsVersion::new(0) => arbos_test;
    "ArbOwnerPublic", 0x6b, ArbOsVersion::new(0) => arb_owner_public;
    "ArbAggregator", 0x6d, ArbOsVersion::new(0) => arb_aggregator;
    "ArbRetryableTx", 0x6e, ArbOsVersion::new(0) => arb_retryable_tx;
    "ArbStatistics", 0x6f, ArbOsVersion::new(0) => arb_statistics;
    "ArbOwner", 0x70, ArbOsVersion::new(0) => arb_owner;
    "ArbWasm", 0x71, ArbOsVersion::V30 => arb_wasm;
    "ArbWasmCache", 0x72, ArbOsVersion::V30 => arb_wasm_cache;
    "ArbNativeTokenManager", 0x73, ArbOsVersion::V41 => arb_native_token_manager;
    "ArbFilteredTransactionsManager", 0x74, ArbOsVersion::V60 => arb_filtered_transactions_manager;
}

/// `ArbosActs`, ArbOS's entry points for internal transactions.
fn arbos_acts(
    _evm: &mut ArbEvm<'_>,
    _message: &Message<ArbEvmTypes>,
    _gas: &mut GasTracker,
) -> PrecompileResult {
    Err(PrecompileError::fatal(ArbOsPrecompileUnsupported {
        precompile: "ArbosActs",
        method: None,
    }))
}

/// Returns the Arbitrum precompile map for an ArbOS version.
pub fn arbitrum_precompile_map(spec: ArbOsVersion) -> PrecompileMap<ArbEvmTypes> {
    let mut map = if spec.enables(ArbOsVersion::V50) {
        Precompiles::<ArbEvmTypes>::base(SpecId::OSAKA).as_map().clone()
    } else if spec.enables(ArbOsVersion::V30) {
        let mut map = Precompiles::<ArbEvmTypes>::base(SpecId::CANCUN).as_map().clone();
        map.insert(P256VERIFY());
        map
    } else {
        Precompiles::<ArbEvmTypes>::base(SpecId::BERLIN).as_map().clone()
    };
    map.extend([
        Precompile::new(ARB_SYS_ADDRESS, PrecompileId::custom("ArbSys"), arb_sys),
        Precompile::new(ARB_INFO_ADDRESS, PrecompileId::custom("ArbInfo"), arb_info),
        Precompile::new(ARB_GAS_INFO_ADDRESS, PrecompileId::custom("ArbGasInfo"), arb_gas_info),
        Precompile::new(ARB_DEBUG_ADDRESS, PrecompileId::custom("ArbDebug"), arb_debug),
        Precompile::new(ARBOS_ADDRESS, PrecompileId::custom("ArbosActs"), arbos_acts),
    ]);
    map.extend(
        unsupported_precompiles()
            .filter(|(since, _)| spec.enables(*since))
            .map(|(_, precompile)| precompile),
    );
    map
}

/// Returns the Arbitrum precompile provider for an ArbOS version.
pub fn arbitrum_precompiles(spec: ArbOsVersion) -> Precompiles<ArbEvmTypes> {
    Precompiles::new(Cow::Owned(arbitrum_precompile_map(spec)))
}
