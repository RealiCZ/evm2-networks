//! Differential parity tests for the Base precompile set.
//!
//! Compares the installed addresses and the execution outcome (status, gas used, and output) of
//! every precompile in `base-evm2`'s [`BasePrecompiles`] against the revm-based
//! `base_common_precompiles::BasePrecompiles` static table, for every Base upgrade.

mod common;

use alloy_primitives::{Address, Bytes};
use base_common_genesis::BaseUpgrade;
use base_evm2::{
    BaseEvmFactory, BaseEvmTypes, BasePrecompiles, BaseSpecId, base_version, precompiles,
};
use evm2::{
    env::BlockEnv,
    evm::{InMemoryDB, precompile::PrecompileProvider},
    interpreter::{GasTracker, Message},
    precompiles::PrecompileError,
};
use revm::precompile::PrecompileStatus;

const ALL: [BaseUpgrade; 16] = [
    BaseUpgrade::Bedrock,
    BaseUpgrade::Regolith,
    BaseUpgrade::Canyon,
    BaseUpgrade::Delta,
    BaseUpgrade::Ecotone,
    BaseUpgrade::Fjord,
    BaseUpgrade::Granite,
    BaseUpgrade::Holocene,
    BaseUpgrade::PectraBlobSchedule,
    BaseUpgrade::Isthmus,
    BaseUpgrade::Jovian,
    BaseUpgrade::Azul,
    BaseUpgrade::Beryl,
    BaseUpgrade::Cobalt,
    BaseUpgrade::Denim,
    BaseUpgrade::Zenith,
];

const GAS_LIMIT: u64 = 1 << 40;

#[derive(Debug, PartialEq, Eq)]
enum Run {
    Success { gas_used: u64, output: Bytes },
    Revert { gas_used: u64, output: Bytes },
    Halt,
}

fn revm_table(upgrade: BaseUpgrade) -> &'static revm::precompile::Precompiles {
    base_common_precompiles::BasePrecompiles::<BaseUpgrade>::new_with_spec(upgrade).precompiles()
}

fn run_revm(upgrade: BaseUpgrade, address: Address, input: &[u8]) -> Run {
    let precompile = revm_table(upgrade).get(&address).expect("installed in revm");
    match precompile.execute(input, GAS_LIMIT, 0) {
        Ok(output) => match output.status {
            PrecompileStatus::Success => {
                Run::Success { gas_used: output.gas_used, output: output.bytes }
            }
            PrecompileStatus::Revert => {
                Run::Revert { gas_used: output.gas_used, output: output.bytes }
            }
            PrecompileStatus::Halt(_) => Run::Halt,
        },
        Err(_) => Run::Halt,
    }
}

fn run_evm2(upgrade: BaseUpgrade, address: Address, input: &[u8]) -> Run {
    let spec = BaseSpecId::new(upgrade);
    let mut evm = BaseEvmFactory::default().create_evm(
        spec,
        base_version(spec, common::CHAIN_ID),
        BlockEnv::<BaseEvmTypes>::default(),
        InMemoryDB::default(),
    );
    let mut precompiles = BasePrecompiles::new(spec);
    let message = Message::<BaseEvmTypes> {
        input: Bytes::copy_from_slice(input),
        code_address: address,
        destination: address,
        call_target: address,
        ..Default::default()
    };
    let mut gas = GasTracker::new(GAS_LIMIT);
    match precompiles.execute(&mut evm, &message, &mut gas).expect("installed in evm2") {
        Ok(output) => {
            Run::Success { gas_used: GAS_LIMIT - gas.remaining(), output: output.into_bytes() }
        }
        Err(PrecompileError::Revert(output)) => {
            Run::Revert { gas_used: GAS_LIMIT - gas.remaining(), output }
        }
        Err(PrecompileError::Halt(_)) => Run::Halt,
        Err(error) => panic!("fatal precompile error: {error}"),
    }
}

fn sorted(addresses: impl IntoIterator<Item = Address>) -> Vec<Address> {
    let mut addresses: Vec<_> = addresses.into_iter().collect();
    addresses.sort();
    addresses
}

/// Beryl and Cobalt stateful precompiles the reference installs that are not ported yet.
const NOT_PORTED: [Address; 3] = [
    precompiles::POLICY_REGISTRY_ADDRESS,
    precompiles::B20_FACTORY_ADDRESS,
    precompiles::TX_CONTEXT_ADDRESS,
];

#[test]
fn installed_addresses_match_revm_reference() {
    for upgrade in ALL {
        let evm2 = sorted(BasePrecompiles::new(BaseSpecId::new(upgrade)).addresses());
        let installed =
            base_common_precompiles::BasePrecompiles::<BaseUpgrade>::new_with_spec(upgrade)
                .install();
        let revm = sorted(installed.addresses().copied().filter(|a| !NOT_PORTED.contains(a)));
        assert_eq!(evm2, revm, "precompile addresses diverged at {upgrade:?}");
    }
}

#[test]
fn small_inputs_match_revm_reference() {
    let mut inputs = vec![vec![], vec![0u8; 32], vec![0u8; 128], vec![0u8; 192], vec![1u8; 200]];
    // A valid modexp input: base 3, exponent 5, modulus 7, each one byte.
    let mut modexp = vec![0u8; 96];
    modexp[31] = 1;
    modexp[63] = 1;
    modexp[95] = 1;
    modexp.extend([3, 5, 7]);
    inputs.push(modexp);
    for upgrade in ALL {
        let addresses = sorted(BasePrecompiles::new(BaseSpecId::new(upgrade)).addresses());
        // Stateful calls are covered by `activation_parity` and `standard_parity`.
        for address in addresses.into_iter().filter(|a| {
            ![precompiles::ACTIVATION_REGISTRY_ADDRESS, precompiles::NONCE_MANAGER_ADDRESS]
                .contains(a)
        }) {
            for input in &inputs {
                assert_eq!(
                    run_evm2(upgrade, address, input),
                    run_revm(upgrade, address, input),
                    "precompile {address} diverged at {upgrade:?} for a {}-byte input",
                    input.len()
                );
            }
        }
    }
}

#[test]
fn input_caps_match_revm_reference() {
    let caps = [
        (
            BaseUpgrade::Granite,
            precompiles::BN254_PAIR_ADDRESS,
            precompiles::BN254_PAIR_GRANITE_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Jovian,
            precompiles::BN254_PAIR_ADDRESS,
            precompiles::BN254_PAIR_JOVIAN_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Isthmus,
            precompiles::BLS12_G1_MSM_ADDRESS,
            precompiles::BLS12_G1_MSM_ISTHMUS_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Jovian,
            precompiles::BLS12_G1_MSM_ADDRESS,
            precompiles::BLS12_G1_MSM_JOVIAN_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Isthmus,
            precompiles::BLS12_G2_MSM_ADDRESS,
            precompiles::BLS12_G2_MSM_ISTHMUS_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Jovian,
            precompiles::BLS12_G2_MSM_ADDRESS,
            precompiles::BLS12_G2_MSM_JOVIAN_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Isthmus,
            precompiles::BLS12_PAIRING_ADDRESS,
            precompiles::BLS12_PAIRING_ISTHMUS_MAX_INPUT_SIZE,
        ),
        (
            BaseUpgrade::Jovian,
            precompiles::BLS12_PAIRING_ADDRESS,
            precompiles::BLS12_PAIRING_JOVIAN_MAX_INPUT_SIZE,
        ),
    ];
    for (upgrade, address, cap) in caps {
        let over = vec![0u8; cap + 1];
        assert_eq!(run_evm2(upgrade, address, &over), Run::Halt, "{address} at {upgrade:?}");
        assert_eq!(run_revm(upgrade, address, &over), Run::Halt, "{address} at {upgrade:?}");
    }
    // At the tightest (Jovian) caps the full input is processed, identically on both engines.
    for (upgrade, address, cap) in caps.into_iter().filter(|(u, ..)| *u == BaseUpgrade::Jovian) {
        let max = vec![0u8; cap];
        let evm2 = run_evm2(upgrade, address, &max);
        assert!(matches!(evm2, Run::Success { .. }), "{address} must accept {cap} bytes");
        assert_eq!(evm2, run_revm(upgrade, address, &max), "{address} at {upgrade:?}");
    }
}

#[test]
fn cap_constants_match_revm_reference() {
    use base_common_precompiles as reference;
    assert_eq!(precompiles::BN254_PAIR_GRANITE_MAX_INPUT_SIZE, reference::GRANITE_MAX_INPUT_SIZE);
    assert_eq!(precompiles::BN254_PAIR_JOVIAN_MAX_INPUT_SIZE, reference::JOVIAN_MAX_INPUT_SIZE);
    assert_eq!(
        precompiles::BLS12_G1_MSM_ISTHMUS_MAX_INPUT_SIZE,
        reference::ISTHMUS_G1_MSM_MAX_INPUT_SIZE
    );
    assert_eq!(
        precompiles::BLS12_G1_MSM_JOVIAN_MAX_INPUT_SIZE,
        reference::JOVIAN_G1_MSM_MAX_INPUT_SIZE
    );
    assert_eq!(
        precompiles::BLS12_G2_MSM_ISTHMUS_MAX_INPUT_SIZE,
        reference::ISTHMUS_G2_MSM_MAX_INPUT_SIZE
    );
    assert_eq!(
        precompiles::BLS12_G2_MSM_JOVIAN_MAX_INPUT_SIZE,
        reference::JOVIAN_G2_MSM_MAX_INPUT_SIZE
    );
    assert_eq!(
        precompiles::BLS12_PAIRING_ISTHMUS_MAX_INPUT_SIZE,
        reference::ISTHMUS_PAIRING_MAX_INPUT_SIZE
    );
    assert_eq!(
        precompiles::BLS12_PAIRING_JOVIAN_MAX_INPUT_SIZE,
        reference::JOVIAN_PAIRING_MAX_INPUT_SIZE
    );
}

#[test]
fn registry_addresses_match_revm_reference() {
    use base_common_precompiles as reference;
    assert_eq!(
        precompiles::ACTIVATION_REGISTRY_ADDRESS,
        reference::ActivationRegistryStorage::ADDRESS
    );
    assert_eq!(precompiles::POLICY_REGISTRY_ADDRESS, reference::PolicyRegistryStorage::ADDRESS);
    assert_eq!(precompiles::B20_FACTORY_ADDRESS, reference::B20FactoryStorage::ADDRESS);
    assert_eq!(precompiles::TX_CONTEXT_ADDRESS, reference::TxContextStorage::ADDRESS);
    assert_eq!(precompiles::NONCE_MANAGER_ADDRESS, reference::NonceManagerStorage::ADDRESS);
}

#[test]
fn activation_admin_matches_chain_config() {
    use base_common_chains::ChainConfig;
    for chain_id in [1, 10, 8453, 84532, 763360, 84538453, 31337] {
        for upgrade in ALL {
            assert_eq!(
                precompiles::activation_admin_address_for_upgrade_by_chain_id(chain_id, upgrade),
                ChainConfig::activation_admin_address_for_upgrade_by_chain_id(chain_id, upgrade),
                "chain {chain_id} at {upgrade:?}"
            );
        }
    }
}

#[test]
fn factory_installs_precompiles_with_activation_admin() {
    let admin = Address::repeat_byte(0xaa);
    let factory = BaseEvmFactory::default().with_activation_admin_address(Some(admin));
    for (upgrade, state_enabled) in [(BaseUpgrade::Beryl, false), (BaseUpgrade::Cobalt, true)] {
        let spec = BaseSpecId::new(upgrade);
        let precompiles = factory.precompiles(spec);
        assert_eq!(precompiles.activation_admin_address(), Some(admin));
        assert_eq!(precompiles.activation_admin().state_enabled, state_enabled);

        let mut evm = factory.create_evm(
            spec,
            factory.version(spec, 8453),
            BlockEnv::<BaseEvmTypes>::default(),
            InMemoryDB::default(),
        );
        let installed = evm.precompiles_as::<BasePrecompiles>().expect("Base precompiles");
        assert_eq!(installed.activation_admin_address(), Some(admin));
        assert_eq!(installed.spec(), spec);

        // `configure_evm` reinstalls the precompiles for the EVM's spec.
        evm.set_precompiles(evm2::precompiles::Precompiles::<BaseEvmTypes>::base(spec.into()));
        BaseEvmFactory::default().configure_evm(&mut evm);
        let installed = evm.precompiles_as::<BasePrecompiles>().expect("Base precompiles");
        assert_eq!(installed.activation_admin_address(), None);
        assert!(installed.contains(&Address::with_last_byte(0x01)));
    }
}
