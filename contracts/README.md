# Contract artifacts

`rns_v3.wasm` is the exact wasm deployed as the RNS v3 contract.

| Item | Value |
|---|---|
| Contract id | `dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77` |
| Code hash (BLAKE3) | `ddf2690c18a55dc38b6489a1cbd7ddc3588bb863d05f4856aff7d314687d8db9` |
| Size | 81,388 bytes |
| Deploy tx | `tx_37f9b3dbeb6f35167aaf015d29518cf9` |
| Deployer / admin | `rete4203660202c54aeea6d540c2063eb928a049201dbf50a50123b7ab7dd608c44da641` |
| Source | `contract/examples/rns_v3/src/lib.rs` |

Check it against the chain:

```bash
retium verify dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77 contracts/rns_v3.wasm
```

The file can also be re-downloaded from the deploy transaction, if this copy is ever lost:

```bash
curl -s https://rpc.retium.org/api/transaction/tx_37f9b3dbeb6f35167aaf015d29518cf9/raw
```

The response is JSON with the wasm base64-encoded in `payload`.

## Check the RTM held by the contract

Contracts have an id, not a wallet address, so `retium balance <id>` rejects it. The RPC still exposes the contract's account, keyed by the same id:

```powershell
(Invoke-RestMethod "https://rpc.retium.org/api/wallet/dbc99e8693aadc3b3aba03860858d76784692e9f19cd036c4b7c7265ce42cc77").balance_rtm
```

At the time of writing this returns `0`, which is the expected state: registration fees go to the treasury and sale proceeds go to the seller, so the contract keeps no balance after a normal call.

