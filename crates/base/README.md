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
  Beryl activation registry driven by the activation admin configuration and the Cobalt
  nonce-manager `getNonce(address,uint256)` getter at `0x813000000000000000000000000000000000aa01`.
- [`BaseEvmFactory`] builds configured [`BaseEvm`] instances.

The handlers honor evm2's [`EvmFeatures`](evm2::EvmFeatures) like the stock Ethereum handlers.
With `FEE_CHARGE` disabled, no gas, L1 data fee, or operator fee is charged or refunded.

## Not ported yet

- The Beryl B-20 factory and tokens and policy registry. They are built on
  `base-precompile-storage`, which is tied to revm.
- The Cobalt transaction-context precompile. Its ordinary-transaction getters require
  `tx.origin`, but evm2's precompile interface does not expose the active `TxEnv`. Caching an
  origin in the transaction handler would not cover raw message execution or system calls.
- EIP-8130 execution (Zenith only, never scheduled on a real chain). The envelope can carry an
  EIP-8130 transaction, but executing it returns an unsupported-transaction-type error.
  Nonce mutation and expiring-nonce replay protection belong to that execution layer; the
  nonce-manager precompile only exposes the read-only getter, as in the reference.
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
