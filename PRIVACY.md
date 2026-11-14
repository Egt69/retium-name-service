# Privacy Policy

**Retium Name Service (RNS)**

**Effective date:** 9 November 2026  
**Last updated:** 9 November 2026

> **Independent project notice:** RNS is an independent project built on the Retium testnet. It is **not** an official Retium project and is not affiliated with, endorsed by, sponsored by, or operated by Retium, the Retium Foundation, or the Retium core team. "Retium" is used only to describe the network. Read the full [Disclaimer](DISCLAIMER.md).

**Use is voluntary and at your own risk.** If you try RNS, you accept the risk of data leaks, real financial loss, lost names, wallet compromise, user error, and network or third-party failures. RNS does not promise to restore data or names or to cover any loss. See [Terms of Use, Section 11](TERMS.md#11-risk-and-responsibility).

---

## 1. Who we are

Retium Name Service ("RNS", "we", "us", or "our") is a non-custodial naming and marketplace application for the Retium network. The application lets users register, renew, update, transfer, list, and purchase `.rtm` names through a self-custody wallet.

RNS is an independent project maintained by a-kai. You can reach us on Discord at `a_kai.me` or on X at `@cryptoyef`.

## 2. Scope

This policy applies to:

- The RNS website and web application.
- Communications with us about RNS.

This policy does not apply to:

- The Retium network itself.
- Third-party wallets, including the Retium Web Wallet.
- Third-party RPC providers, explorers, hosting providers, or links to other sites.
- Smart contracts or public blockchain data that we do not control.

Those services have their own privacy policies. We encourage you to read them.

## 3. Summary

| Data | Why we use it | Where it comes from |
|---|---|---|
| Wallet address | Connect the wallet, read ownership, prepare transactions | Your wallet |
| On-chain activity | Show domains, listings, and history | Public Retium blockchain |
| Domain records | Display names, targets, avatars, and marketplace data | You and the public chain |
| Local cache and history | Keep the app responsive and show recent activity | Your browser storage |
| Transaction identifiers | Show transaction status and link to an explorer | Your wallet and the RPC |
| Technical request data | Deliver the app, prevent abuse, troubleshoot | Your browser and service providers |
| Communications | Answer support and privacy requests | You |

We do not sell personal data. We do not ask for seed phrases or private keys.

## 4. Data we collect

### 4.1 Wallet address

When you connect a wallet, the wallet returns a public address such as `rete...`. We use it to:

- Display your connected account.
- Read the domains you own.
- Prepare registration, renewal, listing, purchase, and transfer transactions.
- Determine whether you are the owner of a name.

A wallet address is public blockchain data. Anyone can see it and the activity associated with it.

### 4.2 On-chain data

RNS reads public data from the Retium blockchain, including:

- Domain ownership.
- Expiry timestamps.
- Resolution targets and avatars.
- Sale prices and listing status.
- Registration, renewal, transfer, and sale events.
- Transaction identifiers and block data.

This data is public, permanent, and may be replicated by nodes and indexers around the world. We cannot delete or change information that has been written to the blockchain.

### 4.3 Local storage and cache

The application stores limited data in your browser's `localStorage`, including:

- `rns_history`: a local list of actions such as registration, renewal, listing, transfer, and purchase.
- `rns_db`: a cache of domain information.
- `rns_db_version`: the cache format version.

This data stays in your browser unless you clear it. It is not a server-side account and is not shared with us automatically. If you use a different browser or device, the cache may not be present, but the on-chain data remains available.

You can clear this data through your browser settings. Clearing it may remove local history and cached information, but it does not affect your domains or on-chain transactions.

### 4.4 Technical data

When your browser loads the application or calls the RPC, technical data may be processed by the providers involved, including:

- IP address.
- Browser type, version, and language.
- Operating system and device type.
- Requested URL, referrer, and timestamp.
- Error and diagnostic information.
- Approximate region derived from IP address.

The RNS application does not currently operate its own analytics or advertising system. However, the infrastructure used to deliver the app may process technical data. See Section 6.

### 4.5 Wallet provider data

The Retium Web Wallet is a separate service. It may process your addresses, permissions, transaction approvals, and device data under its own privacy policy. RNS does not control the wallet and does not receive your private keys.

### 4.6 Fonts and external resources

The landing page and application load fonts from Google Fonts. Your browser connects to Google's servers to fetch those resources, which may involve your IP address and user agent. Google's privacy policy applies to that processing.

## 5. Cookies and similar technologies

RNS does not currently use advertising cookies or third-party analytics cookies.

The application uses browser `localStorage` for the cache and local history described in Section 4.3. Local storage is not a cookie, but it is a similar client-side storage technology.

Third-party services loaded by the app, such as the wallet SDK or font provider, may use their own cookies or storage. Their policies apply to those technologies.

You can block or clear cookies and local storage in your browser, but parts of the app may stop working.

## 6. How we use data

We use data to:

- Provide the RNS application and display on-chain state.
- Connect a wallet and prepare transactions.
- Read ownership, expiry, records, listings, and history.
- Process registrations, renewals, transfers, listings, and purchases.
- Calculate and display prices and fees.
- Respond to support and privacy requests.
- Maintain security, troubleshoot errors, and prevent misuse.

## 7. Sharing and disclosure

We may share data with:

- **Service providers.** Hosting, RPC, wallet, infrastructure, security, and support providers that process data on our behalf.
- **Transaction counterparties.** Public blockchain data is visible to anyone, including buyers, sellers, and observers.

We do not sell personal data. We do not share private keys because we never receive them.

## 8. Data location

RNS is used from many places. Service providers may process data in countries other than your own, and their own practices apply.

Public blockchain data is replicated globally by design. It may be accessible from anywhere.

## 9. Retention

We keep data only as long as needed for the purposes described in this policy:

- **Local cache and history:** until you clear your browser storage or the cache is replaced.
- **Support records:** for the period needed to resolve the matter.
- **Security and technical logs:** for a limited period, unless needed for an investigation.

On-chain data is permanent. We cannot delete or modify it.

## 10. Security

RNS is built to keep data exposure low:

- Non-custodial design with no private keys on RNS servers.
- Wallet-signed transactions.
- Validation in the smart contract.
- Least-privilege access for operational systems.

No system is completely secure. You are responsible for protecting your wallet, devices, passwords, recovery phrase, and signing approvals.

Never share your seed phrase or private key. RNS will never ask for them.

## 11. Your choices

You can:

- Ask what data we hold about you.
- Ask us to correct inaccurate data.
- Ask us to delete data we control, where possible.
- Clear local cache and history through your browser settings.
- Contact us with any privacy question or request.

To make a request, contact us on Discord at `a_kai.me` or on X at `@cryptoyef`. We may ask you to confirm that you control the wallet.

Some things we cannot do:

- We cannot delete public blockchain data.
- We may not be able to identify you from a wallet address alone.

## 12. Children

RNS is not directed to children. We do not knowingly collect personal data from children. If you believe a child has provided personal data, contact us and we will take appropriate action.

## 13. Third-party links and services

The app may link to wallets, explorers, RPC providers, social channels, or other websites. We do not control those services and are not responsible for their privacy practices. Read their policies before using them.

## 14. Changes to this policy

We may update this page when the app or our practices change. The latest version is always shown here with a new effective date.

## 15. Contact

For privacy questions or requests:

- **Discord:** `a_kai.me`
- **X:** `@cryptoyef`

For security issues, contact us through the same channels. Do not include a seed phrase or private key in any message.
