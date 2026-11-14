# RNS v3 contract

The Rust source of the contract running at

```
dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77
```

on the Retium testnet. Retium has no source registry, so the proof that the
chain runs this code is a hash: rebuild the wasm and compare it with the code
hash the chain reports.

## Verify the running code

The deployed bytes are checked in at `contracts/rns_v3.wasm` in this repository, so the check below needs no build. Run it from the repository root:

```bash
retium verify dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77 contracts/rns_v3.wasm
```

To rebuild from this directory (`contract/`) and compare:

```bash
cargo build --release --locked --target wasm32-unknown-unknown -p rns_v3
retium verify dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77 \
  target/wasm32-unknown-unknown/release/rns_v3.wasm
```

Both commands print `match` and the code hash

```
ddf2690c18a55dc38b6489a1cbd7ddc3588bb863d05f4856aff7d314687d8db9
```

The build is pinned to rustc `1.91.1` (`rust-toolchain.toml`) and to the exact
dependency versions in `Cargo.lock`, and the release profile is fixed in
`Cargo.toml` (`opt-level = "z"`, fat LTO, one codegen unit, `panic = "abort"`,
stripped). No extra flags are needed and the build does not depend on the
absolute path of this directory: a copy at another location produces the same
81,388 bytes.

## Layout

```text
.
|-- Cargo.toml                  workspace manifest (trimmed copy of the SDK's)
|-- Cargo.lock
|-- rust-toolchain.toml         pins rustc 1.91.1
|-- LICENSE                     Apache-2.0
|-- crates/retium-contract/     Retium SDK contract crate, unmodified
`-- examples/rns_v3/            the contract
```

`crates/retium-contract` and `examples/rns_v3` are copied unchanged from
[Retium-Org/retium-sdk](https://github.com/Retium-Org/retium-sdk) at commit
`cb23475`, including the host test harness (`retium_contract::test_host`) that
the unit tests use. The crate is Apache-2.0, see `LICENSE`.

The two directory names are part of the build, not decoration. Cargo passes a
workspace member's sources to rustc as paths relative to the workspace root, and
those strings end up inside the wasm (the panic locations, and through them the
order of some symbols). The deployed binary contains `examples\rns_v3\src\lib.rs`
and `crates\retium-contract\src\*.rs`. Renaming either directory, or building
the same source as a standalone package, produces a different hash even though
the code is identical.

## Tests

```bash
cargo test
```

27 tests for the contract and 38 for the SDK crate. They run on the host, so
they need a native linker (MSVC on Windows, cc on Linux), but they need no node:
the SDK crate provides an in-memory chain (`retium_contract::test_host`) with
controllable block time, balances and cross-contract replies.

The contract tests cover name validation and reserved names, the pricing table,
register/renew payment and refunds, the grace period down to the millisecond,
the 30-day listing window, direct payment to the seller on a sale, and rollback
when an RTM payout is rejected.

## Notes

- The contract is immutable. Changing the admin, pricing, grace period, or
  listing rules requires a new deployment and a new contract id.
- `contract_id` is the contract's account id, so it holds RTM like any account.
  Registration fees go to the treasury and sale proceeds go to the seller, so a
  normal call leaves the contract with no balance.
