//! OP Stack constants.
//!
//! Port of `op-revm/src/constants.rs`.

use alloy_primitives::{Address, U256, address};

/// The cost of a non-zero byte in the EVM.
pub const NON_ZERO_BYTE_COST: u64 = 16;

/// Byte offset within the Ecotone fee-scalars slot of the 4-byte `baseFeeScalar` attribute.
pub const BASE_FEE_SCALAR_OFFSET: usize = 16;
/// Byte offset within the Ecotone fee-scalars slot of the 4-byte `blobBaseFeeScalar` attribute.
pub const BLOB_BASE_FEE_SCALAR_OFFSET: usize = 20;

/// Byte offset within the operator-fee slot of the 4-byte `operatorFeeScalar` attribute.
pub const OPERATOR_FEE_SCALAR_OFFSET: usize = 20;
/// Byte offset within the operator-fee slot of the 8-byte `operatorFeeConstant` attribute.
pub const OPERATOR_FEE_CONSTANT_OFFSET: usize = 24;

/// Byte offset within the Jovian slot of the 2-byte `daFootprintGasScalar` attribute.
pub const DA_FOOTPRINT_GAS_SCALAR_OFFSET: usize = 18;

/// The fixed point decimal scaling factor associated with the operator fee scalar.
pub const OPERATOR_FEE_SCALAR_DECIMAL: u64 = 1_000_000;

/// The Jovian multiplier applied to the operator fee scalar component.
pub const OPERATOR_FEE_JOVIAN_MULTIPLIER: u64 = 100;

/// The L1 base fee slot.
pub const L1_BASE_FEE_SLOT: U256 = U256::from_limbs([1u64, 0, 0, 0]);
/// The L1 overhead slot.
pub const L1_OVERHEAD_SLOT: U256 = U256::from_limbs([5u64, 0, 0, 0]);
/// The L1 scalar slot.
pub const L1_SCALAR_SLOT: U256 = U256::from_limbs([6u64, 0, 0, 0]);

/// Ecotone slot storing the L1 `blobBaseFee` attribute.
pub const ECOTONE_L1_BLOB_BASE_FEE_SLOT: U256 = U256::from_limbs([7u64, 0, 0, 0]);

/// Ecotone slot storing the packed `baseFeeScalar` and `blobBaseFeeScalar` attributes.
pub const ECOTONE_L1_FEE_SCALARS_SLOT: U256 = U256::from_limbs([3u64, 0, 0, 0]);

/// Isthmus slot storing the packed `operatorFeeScalar` and `operatorFeeConstant` attributes.
pub const OPERATOR_FEE_SCALARS_SLOT: U256 = U256::from_limbs([8u64, 0, 0, 0]);

/// Jovian slot storing the packed `daFootprintGasScalar` attribute.
pub const DA_FOOTPRINT_GAS_SCALAR_SLOT: U256 = U256::from_limbs([8u64, 0, 0, 0]);

/// An empty 64-bit set of scalar values.
pub const EMPTY_SCALARS: [u8; 8] = [0u8; 8];

/// The address of the L1 fee recipient (`L1FeeVault`).
pub const L1_FEE_RECIPIENT: Address = address!("0x420000000000000000000000000000000000001A");

/// The address of the operator fee recipient (`OperatorFeeVault`).
pub const OPERATOR_FEE_RECIPIENT: Address = address!("0x420000000000000000000000000000000000001B");

/// The address of the base fee recipient (`BaseFeeVault`).
pub const BASE_FEE_RECIPIENT: Address = address!("0x4200000000000000000000000000000000000019");

/// The address of the `L1Block` predeploy.
pub const L1_BLOCK_CONTRACT: Address = address!("0x4200000000000000000000000000000000000015");

/// Deposit transaction type.
pub const DEPOSIT_TRANSACTION_TYPE: u8 = 0x7E;
