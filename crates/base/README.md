# base-evm2

Base's execution type family for [evm2](https://github.com/alloy-rs/evm2).

This crate ports the single-transaction execution semantics of the revm-based
`base-common-evm` (at `foundry-rs/base` rev `2aa8bc46e6b4272ef3bf0905e1a5660ee2525e72`) to evm2:

- [`BaseSpecId`] wraps the Base upgrade schedule ([`BaseUpgrade`]) and maps each upgrade to the
  evm2 spec that governs it.
- [`BaseEvmTypes`] is the evm2 type family, with [`BaseConfigSelector`] selecting the opcode
  table. Base does not change opcodes or opcode gas costs.
- [`BaseTxEnvelope`] covers deposits (`0x7e`), the standard Ethereum transaction types, and
  EIP-8130 (`0x79`) transactions.
- [`BaseTxHandlerHooks`] charges and settles the L1 data fee and the Isthmus operator fee and
  credits the L1 fee, base fee, and operator fee vaults.
- [`deposit`] executes deposits, including failed deposits, which are flagged in
  [`BaseTxResultExt::failed_deposit`].
- [`BasePrecompiles`] is the Base precompile set with its upgrade activation rules, including the
  Beryl activation registry driven by the activation admin configuration, the Beryl B-20 factory,
  B-20 tokens, and policy registry, and the Cobalt nonce-manager and transaction-context getters.
- [`BaseEvmFactory`] builds configured [`BaseEvm`] instances.

The standard transaction handlers honor evm2's [`EvmFeatures`](evm2::EvmFeatures) like the stock
Ethereum handlers. With `FEE_CHARGE` disabled, no gas, L1 data fee, or operator fee is charged or
refunded. Signed EIP-8130 inclusion always applies its protocol authentication and fee checks,
matching Base's reference executor; unsigned RPC simulation is a separate envelope variant.

## Beryl B-20 and policy precompiles

The B-20 factory (`0xB20F…`), the B-20 tokens, and the policy registry (`0x8453…0002`) run Base's
own storage dispatchers from `base-common-precompiles` (`B20FactoryStorage::dispatch`, the asset
and stablecoin token dispatchers, and `PolicyRegistryStorage::dispatch`) rather than a port of
their business logic. A crate-internal `PrecompileStorageProvider` implementation backs those
dispatchers with evm2 state: storage, transient storage, logs, account and code access, and journal
checkpoints map to evm2 state operations and native evm2 rollback, with the reference gas schedule,
refunds, static-call and `DELEGATECALL`/`CALLCODE` rejection, and code-marker charging. revm data
types cross that boundary, so revm is compiled as a dependency, but it never executes. Database
failures stay fatal.

B-20 tokens are resolved by address prefix from Beryl on, like the reference `BerylLookup`, after
the fixed precompile table, so custom precompiles and relocations keep precedence. Token addresses
are not prewarmed. The factory itself stays code-less.

## Transaction context and EIP-8130

The Cobalt transaction-context precompile (`0x8130…aa02`) reads the active precompile's `TxEnv`
for ordinary transactions, raw messages, and system calls. Zenith EIP-8130 preparation publishes
the authenticated sender, payer, and acting actor through Base's transient transaction-context
storage. No origin is cached across executions.

Zenith's EIP-8130 handler shares Base's authentication, account-change, nonce, and intrinsic-gas
rules. Calls execute on evm2 in atomic phases: a failed phase rolls back its writes and skips
later phases while retaining prior phases, consuming the nonce, and charging the payer. Receipts
carry one status byte per phase. Unsigned simulation skips authentication, nonce advancement,
and fees, searches a verified-feasible gas pool without exposing probes to inspectors, and
discards all state even if its caller commits the result.

## Not ported yet

- Block-level pieces: the block executor and the Canyon, Denim, and Zenith irregular state
  transitions.

## Known differences from `base-common-evm`

- With fee charging enabled and an empty L1-fee payload, the reference refunds the operator fee
  on the unused gas although it charged none. This port refunds only what it charged.
- With balance checks disabled, evm2's balance top-up (`EvmFeatures::BALANCE_TOP_UP`) also covers
  the L1 data and operator fees, where the reference rejects a sender that cannot pay them.

## Credits

The structure of this crate, and parts of its code and tests, come from the Base team's
`base-common-evm2` scaffold in [base/base](https://github.com/base/base) (MIT licensed), brought
forward to the evm2 revision pinned by this workspace.
