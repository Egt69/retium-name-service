# Retium Name Service (RNS)

**Human-readable names for the Retium network.**

Retium Name Service is a non-custodial naming and marketplace protocol built on Retium. It maps readable names such as `alice.rtm` to an owner, a resolution target, and optional profile data. Ownership, expiry, listings, and transfers live on-chain.

This repository contains the RNS web application, the public contract configuration, and the project documentation. The v3 contract source and its tests live in the Retium SDK workspace used to build and deploy the contract.

> **Independent project notice:** RNS is an independent project built on the Retium testnet. It is **not** an official Retium project and is not affiliated with, endorsed by, sponsored by, or operated by Retium, the Retium Foundation, or the Retium core team. "Retium" is used only to describe the network. Read the full [Disclaimer](DISCLAIMER.md).

---

## Contents

- [Overview](#overview)
- [Why RNS](#why-rns)
- [Core features](#core-features)
- [How it works](#how-it-works)
- [Contract information](#contract-information)
- [Repository layout](#repository-layout)
- [Local development](#local-development)
- [Testing](#testing)
- [Security model](#security-model)
- [Roadmap](#roadmap)
- [Documentation](#documentation)

---

## Overview

RNS gives every Retium wallet a readable identity. Instead of sharing a long `rete...` address, a user can register a name, set a resolution target, and trade or transfer the name through an on-chain marketplace.

The application is a static frontend. It talks directly to the Retium RPC and to the Retium Web Wallet. There is no account system, no custodian, and no server-side database controlled by RNS.

Key properties:

- **Non-custodial.** RNS never holds private keys or user funds.
- **On-chain ownership.** The contract is the source of truth for owner, expiry, records, and listings.
- **Deterministic rules.** Pricing, validation, grace periods, and sale rules are enforced by the contract.
- **Portable identity.** The same wallet sees the same domains on every browser and device.
- **Open verification.** Contract id, code hash, and test coverage are published for review.

## Why RNS

Long wallet addresses are hard to read, verify, and remember. A naming layer makes payments, profiles, and discovery simpler:

- Send to `alice.rtm` instead of a 72-character address.
- Give a wallet a public identity with a target and avatar.
- List names on a native marketplace without an escrow service.
- Renew, transfer, and update records with transparent on-chain rules.

## Core features

### Registration

- Names are 3 to 10 characters: lowercase letters, digits, and hyphens.
- Only the `.rtm` extension is supported.
- Reserved names are rejected by the contract.
- The registration fee is calculated on-chain and paid to the treasury.
- Overpayment is refunded automatically.

### Renewal

- Owners can renew for 3, 6, 12, or 36 months.
- Renewal is allowed while the name is active or inside its 30-day grace period.
- A renewal during grace starts from the current block time, not the old expiry.
- Only the owner can renew.

### Records

- The owner can set a resolution target and an avatar.
- Targets may be a valid Retium address or an `http`, `https`, `ipfs`, `ipns`, or `ar` URL.
- Record updates are rejected after expiry.

### Marketplace

- Owners can list a name at a fixed price in RTM.
- A listing requires at least 30 days of remaining registration.
- A buyer pays the seller directly through the contract.
- The buyer receives any excess payment back.
- The contract checks every RTM transfer reply and fails the whole call if a payout is rejected.
- Listings are indexed with a linked list and paginated, so a long history does not turn into a full scan.

### Transfer

- The owner can transfer an active name to another valid Retium address.
- The recipient must be a valid `rete` address with 68 lowercase hexadecimal characters.
- A transfer clears any active listing.

### Read API

- `get_info` returns owner, target, avatar, sale price, expiry, and status.
- `get_domains_by_owner` and `get_listed_domains` support cursor pagination.
- `quote` returns the exact atomic price for a name and duration.
- `is_reserved` reports whether a name is reserved.
- `get_config` exposes treasury, admin, grace period, minimum remaining time, and version.

## How it works

1. The user connects a Retium Web Wallet.
2. The app reads contract state from the Retium RPC.
3. The user signs a transaction in the wallet.
4. The wallet submits the transaction to Retium.
5. The contract validates the call, updates state, and pays the treasury or seller.
6. The app refreshes the affected data from the chain.

The frontend keeps a small local cache and transaction history in `localStorage`. The chain remains the source of truth. If the cache and the chain disagree, the chain wins.

## Contract information

| Item | Value |
|---|---|
| Network | Retium testnet |
| Chain id (app config) | `retium-testnet-1` |
| Contract id | `dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77` |
| Code hash | `ddf2690c18a55dc38b6489a1cbd7ddc3588bb863d05f4856aff7d314687d8db9` |
| Contract version | `v3.0.0` |
| Treasury and admin | `rete4203660202c54aeea6d540c2063eb928a049201dbf50a50123b7ab7dd608c44da641` |
| RPC | `https://rpc.retium.org` |
| Wallet SDK | `https://wallet.retium.org/connect/sdk/retium-sdk-1.3.0.js` |

The contract is immutable. A change to the admin, pricing model, grace period, or minimum listing period requires a new deployment and a new contract id.

## Verify the contract

Retium does not host source code. The chain stores the compiled wasm and its code hash, so verification means rebuilding the wasm and comparing hashes. The deployed wasm is checked in at `contracts/rns_v3.wasm`, so anyone can run the check without a build:

```bash
retium verify dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77 contracts/rns_v3.wasm
```

To reproduce those exact bytes from source, build the `contract/` directory, which carries the Rust source and the SDK crate it needs:

```bash
cd contract
cargo build --release --locked --target wasm32-unknown-unknown -p rns_v3
retium verify dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77 `
  target\wasm32-unknown-unknown\release\rns_v3.wasm
```

Both commands must print `match` and the code hash `ddf2690c18a55dc38b6489a1cbd7ddc3588bb863d05f4856aff7d314687d8db9`. The build is pinned to rustc `1.91.1` (`rust-toolchain.toml`) and to `Cargo.lock`, and needs no extra flags on Windows or Linux. See `contract/README.md` for the tests and for the reason the source has to keep the SDK's directory layout.

A match proves that the running code is this source. It does not prove that the source is safe, and the explorer will keep showing "Source not published" because Retium has no source-upload registry — the hash check is the verification.

## Repository layout

```text
.
|-- app.html          Application shell and modals
|-- app.js            Wallet, contract reads, transactions, rendering
|-- config.js         Network, contract id, RPC, history limit
|-- contract/         Rust source of the contract, buildable on its own
|-- contracts/        Deployed wasm and its verification notes
|-- index.html        Landing page
|-- landing.css       Landing page styles
|-- landing.js        Landing page interactions
|-- style.css         Application styles
|-- docs/
|   `-- rns-v3-spec.md
|-- DISCLAIMER.md     Disclaimer
|-- PRIVACY.md        Privacy Policy
|-- TERMS.md          Terms of Use
`-- README.md
```

The contract source lives in `contract/`, laid out like the Retium SDK workspace it was written in (`crates/retium-contract` plus `examples/rns_v3`), so it builds without the SDK checkout. The deployed wasm is 81,388 bytes, its code hash is recorded above, and a copy is kept in `contracts/rns_v3.wasm` for verification.

## Local development

The application has no build step. Serve the directory with any static file server.

```bash
python -m http.server 4173
```

Then open:

```text
http://localhost:4173/
```

To use the application, install the Retium Web Wallet and connect it. Reads use the public RPC. Writes require a funded testnet wallet.

Configuration lives in `config.js`:

```js
const CONFIG = Object.freeze({
    RPC_URL: "https://rpc.retium.org",
    CHAIN_ID: "retium-testnet-1",
    CONTRACT_ID: "dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77",
    EXPLORER_TX_URL: "https://rpc.retium.org/api/transaction",
    HISTORY_LIMIT: 200
});
```

## Testing

Contract-level tests:

- 27 unit tests for the v3 contract.
- 38 unit tests for the Retium contract SDK.
- Integration coverage on testnet for registration, overpayment, renewal, transfer, listings, purchases, treasury rotation, pagination, admin gating, and rejected calls.
- Deterministic host tests for grace-period boundaries, the 30-day listing and purchase limit, direct seller payout, refunds, and RTM error-reply rollback.

Frontend checks:

- `node --check app.js`
- Browser testing against live testnet data for quotes, availability, owner index, marketplace, pagination, status rendering, renewal, and recipient validation.

## Security model

- The frontend never asks for a seed phrase or private key.
- The wallet signs every state-changing call.
- The contract validates names, addresses, durations, ownership, expiry, and payment amounts.
- Every payout checks `call::is_error_reply`; a rejected RTM transfer traps the call.
- Sale proceeds go to the seller, not to the contract.
- The contract has no withdraw path for user funds and is expected to hold a zero balance after normal calls.
- Admin can rotate the treasury and rescue unexpected balances. Admin cannot move a user's domain.

## Roadmap

The following items are planned or under consideration. They are not commitments.

- [ ] Mainnet deployment planning and independent review.
- [ ] Domain migration tooling for earlier testnet users.
- [ ] Public status page and incident process.
- [ ] Additional wallet and explorer integrations.

## Documentation

- [Disclaimer](DISCLAIMER.md)
- [Privacy Policy](PRIVACY.md)
- [Terms of Use](TERMS.md)
- [RNS v3 specification](docs/rns-v3-spec.md)
