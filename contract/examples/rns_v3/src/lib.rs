//! Retium Name Service, version 3.
//!
//! Design notes (see `docs/rns-v3-spec.md` in the app repo):
//!
//! * `ADMIN` is hardcoded. Deploy with the admin key, then call `init` in the
//!   same maintenance window. Until `init` runs nobody can set the treasury.
//! * Every payout checks `call::is_error_reply`. A cross-contract error reply is
//!   not a rollback, so `send_rtm` returning `Ok` is not enough.
//! * Failures after the first state write use `fail()`, which rolls back for
//!   user calls and contract callers alike.
//! * Sale prices and expiries are returned as strings so JavaScript clients do
//!   not lose precision.
//! * Active listings live in a linked list, so a growing history of listings
//!   never turns into a full-prefix scan.
//! * Expired domains have a 30 day grace period for the previous owner to
//!   renew. After that anybody may register the name again.
//! * Domain names are 3 to 10 characters, lowercase ASCII letters, digits and
//!   hyphens.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use retium_contract::{call, entrypoint, env, error, events, fail, ok, storage, Call, Reply};

entrypoint!(handle);

retium_contract::rcp_metadata!(
    r#"{"rcp_type":"domain","traits":["nft","marketplace"],"capabilities":["execute"],"lockable_fields":[],"evolves_from":"12d52af01d7bab294319a539445301589c4f17db7dd4c530a3fe8e3808c9c774","weight":5,"version":"v3.0.0"}"#
);

/// The only address allowed to call `init`, `set_treasury` and `rescue`.
///
/// This is the deployer / multisig. Change it here and redeploy if the key
/// moves; it is deliberately not mutable from storage.
const ADMIN: &str = "rete4203660202c54aeea6d540c2063eb928a049201dbf50a50123b7ab7dd608c44da641";

/// 1 RTM is 10^18 atomic units.
const ATOMIC_PER_RTM: u128 = 1_000_000_000_000_000_000;
const MIN_NAME_LEN: usize = 3;
const MAX_NAME_LEN: usize = 10;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Time after expiry during which only the previous owner may renew.
const GRACE_MS: u64 = 30 * DAY_MS;
/// A listing or purchase needs at least this much life left.
const MIN_REMAINING_MS: u64 = 30 * DAY_MS;
const MAX_TARGET_BYTES: usize = 256;
const MAX_AVATAR_BYTES: usize = 512;
const DEFAULT_PAGE_SIZE: usize = 25;
const MAX_PAGE_SIZE: usize = 50;
/// Stops a corrupt linked list from burning all the fuel of one query.
const MAX_LIST_WALK: usize = 500;
const VERSION: &str = "v3.0.0";

const RESERVED_NAMES: &[&str] = &[
    "admin", "retium", "rtm", "rtd", "www", "api", "mail", "support", "help", "status", "blog",
    "docs", "app", "wallet", "explorer", "treasury", "official", "system", "root", "null",
];

pub fn handle(call: Call) -> Reply {
    match call.action() {
        "init" => init(&call),
        "register" => register(&call),
        "renew" => renew(&call),
        "update_records" => update_records(&call),
        "list_for_sale" => list_for_sale(&call),
        "buy_domain" => buy_domain(&call),
        "transfer_domain" => transfer_domain(&call),
        "set_treasury" => set_treasury(&call),
        "rescue" => rescue(&call),
        "get_info" => get_info(&call),
        "get_domains_by_owner" => get_domains_by_owner(&call),
        "get_listed_domains" => get_listed_domains(&call),
        "quote" => quote(&call),
        "is_reserved" => is_reserved_action(&call),
        "get_config" => get_config(&call),
        other => error(&format!("unknown action '{other}'")),
    }
}

// --- SMALL HELPERS ----------------------------------------------------------

fn now_ms() -> u64 {
    env::block_time_ms().max(0) as u64
}

fn is_active(expires_ms: u64) -> bool {
    expires_ms != 0 && now_ms() <= expires_ms
}

fn in_grace(expires_ms: u64) -> bool {
    expires_ms != 0
        && now_ms() > expires_ms
        && now_ms() <= expires_ms.saturating_add(GRACE_MS)
}

fn remaining_ms(expires_ms: u64) -> u64 {
    expires_ms.saturating_sub(now_ms())
}

fn duration_ms(months: u64) -> u64 {
    months * 30 * DAY_MS
}

fn treasury() -> Result<String, String> {
    let value = storage::get_or("treasury", "")?;
    if value.is_empty() {
        return Err("contract is not initialized: call init with the treasury address".to_string());
    }
    Ok(value)
}

/// Abort after state has been written. `fail` rolls back transfers and events
/// for user calls and contract callers alike.
fn must(result: Result<(), String>) {
    if let Err(reason) = result {
        fail(&reason);
    }
}

fn must_set(key: &str, value: &str) {
    must(storage::set(key, value));
}

/// Send RTM and insist that the ledger accepted it. `send_rtm` returns `Ok`
/// even when the RTM contract answers with an error reply.
fn must_pay(to: &str, amount: u128, what: &str) {
    if amount == 0 {
        return;
    }
    match call::send_rtm(to, amount) {
        Ok(reply) if !call::is_error_reply(&reply) => {}
        Ok(_) => fail(&format!("{what}: RTM transfer was rejected")),
        Err(reason) => fail(&format!("{what}: {reason}")),
    }
}

fn non_payable(call: &Call) -> Option<Reply> {
    if call.value() > 0 {
        Some(error("this action is not payable"))
    } else {
        None
    }
}

// --- VALIDATION -------------------------------------------------------------

fn is_valid_address(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 72 || &bytes[..4] != b"rete" {
        return false;
    }
    bytes[4..].iter().all(|b| {
        (*b >= b'0' && *b <= b'9') || (*b >= b'a' && *b <= b'f')
    })
}

fn is_reserved(name: &str) -> bool {
    RESERVED_NAMES.iter().any(|reserved| *reserved == name)
}

fn validate_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.len() < MIN_NAME_LEN {
        return Err(format!("name must be at least {MIN_NAME_LEN} characters"));
    }
    if bytes.len() > MAX_NAME_LEN {
        return Err(format!("name must be at most {MAX_NAME_LEN} characters"));
    }
    for &byte in bytes {
        let valid =
            (byte >= b'a' && byte <= b'z') || (byte >= b'0' && byte <= b'9') || byte == b'-';
        if !valid {
            return Err("name can only contain lowercase letters, numbers, and hyphens".to_string());
        }
    }
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return Err("name cannot start or end with a hyphen".to_string());
    }
    // Reject the punycode marker `xn--` (and any other `--` at positions 3-4).
    if bytes.len() >= 4 && bytes[2] == b'-' && bytes[3] == b'-' {
        return Err("name cannot contain '--' at positions 3-4".to_string());
    }
    if is_reserved(name) {
        return Err("this name is reserved".to_string());
    }
    Ok(())
}

fn domain_name(domain: &str) -> Result<&str, String> {
    domain
        .strip_suffix(".rtm")
        .ok_or_else(|| "domain must end with .rtm".to_string())
}

fn validate_domain(domain: &str) -> Result<&str, String> {
    let name = domain_name(domain)?;
    validate_name(name)?;
    Ok(name)
}

fn price_for_name(name: &str, months: u64) -> Result<u128, String> {
    let base_per_year: u128 = match name.len() {
        3 => 50,
        4 => 20,
        _ => 5,
    } * ATOMIC_PER_RTM;

    // 36 months is three years at 85%: base * 3 * 85 / 100 = base * 255 / 100.
    let percent: u128 = match months {
        3 => 25,
        6 => 50,
        12 => 100,
        36 => 255,
        _ => return Err("invalid duration (3, 6, 12, 36)".to_string()),
    };
    Ok(base_per_year * percent / 100)
}

fn is_valid_target(target: &str) -> bool {
    if target.is_empty() {
        return true;
    }
    if is_valid_address(target) {
        return true;
    }
    target.starts_with("https://")
        || target.starts_with("http://")
        || target.starts_with("ipfs://")
        || target.starts_with("ipns://")
        || target.starts_with("ar://")
}

fn page_limit(raw: Option<&str>) -> usize {
    match raw.and_then(|value| value.parse::<usize>().ok()) {
        Some(value) if value > 0 => value.min(MAX_PAGE_SIZE),
        _ => DEFAULT_PAGE_SIZE,
    }
}

// --- STORAGE KEYS -----------------------------------------------------------

fn owner_key(domain: &str) -> String {
    format!("owner:{domain}")
}

fn target_key(domain: &str) -> String {
    format!("target:{domain}")
}

fn avatar_key(domain: &str) -> String {
    format!("avatar:{domain}")
}

fn expires_key(domain: &str) -> String {
    format!("expires:{domain}")
}

fn sale_price_key(domain: &str) -> String {
    format!("sale_price:{domain}")
}

fn owned_key(owner: &str, domain: &str) -> String {
    format!("owned:{owner}:{domain}")
}

fn listing_prev_key(domain: &str) -> String {
    format!("listing_prev:{domain}")
}

fn listing_next_key(domain: &str) -> String {
    format!("listing_next:{domain}")
}

fn read_expires(domain: &str) -> Result<u64, String> {
    let raw = storage::get_or(&expires_key(domain), "")?;
    if raw.is_empty() {
        return Ok(0);
    }
    raw.parse()
        .map_err(|_| format!("expiry of {domain} is invalid"))
}

fn read_sale_price(domain: &str) -> Result<String, String> {
    storage::get_or(&sale_price_key(domain), "")
}

// --- LISTING INDEX ----------------------------------------------------------
//
// Only active listings are in the list. Link keys are cleared instead of
// deleted, but readers walk from `listing_head`, never by key prefix.

fn is_linked(domain: &str) -> Result<bool, String> {
    if !storage::get_or(&listing_prev_key(domain), "")?.is_empty() {
        return Ok(true);
    }
    if !storage::get_or(&listing_next_key(domain), "")?.is_empty() {
        return Ok(true);
    }
    Ok(storage::get_or("listing_head", "")? == domain)
}

fn link_listing(domain: &str) -> Result<(), String> {
    let tail = storage::get_or("listing_tail", "")?;
    if tail.is_empty() {
        storage::set("listing_head", domain)?;
    } else {
        storage::set(&listing_next_key(&tail), domain)?;
        storage::set(&listing_prev_key(domain), &tail)?;
    }
    storage::set("listing_tail", domain)?;
    Ok(())
}

fn unlink_listing(domain: &str) -> Result<(), String> {
    let prev = storage::get_or(&listing_prev_key(domain), "")?;
    let next = storage::get_or(&listing_next_key(domain), "")?;
    let head = storage::get_or("listing_head", "")?;
    let tail = storage::get_or("listing_tail", "")?;

    if prev.is_empty() {
        if head == domain {
            storage::set("listing_head", &next)?;
        }
    } else {
        storage::set(&listing_next_key(&prev), &next)?;
    }

    if next.is_empty() {
        if tail == domain {
            storage::set("listing_tail", &prev)?;
        }
    } else {
        storage::set(&listing_prev_key(&next), &prev)?;
    }

    if !prev.is_empty() {
        storage::set(&listing_prev_key(domain), "")?;
    }
    if !next.is_empty() {
        storage::set(&listing_next_key(domain), "")?;
    }
    Ok(())
}

fn index_add_owner(owner: &str, domain: &str) -> Result<(), String> {
    storage::set(&owned_key(owner, domain), "1")
}

fn index_remove_owner(owner: &str, domain: &str) -> Result<(), String> {
    storage::set(&owned_key(owner, domain), "")
}

// --- JSON HELPERS -----------------------------------------------------------

fn json_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&events::json_string(item));
    }
    out.push(']');
    out
}

// --- WRITE ACTIONS ----------------------------------------------------------

pub fn init(call: &Call) -> Reply {
    if let Some(reply) = non_payable(call) {
        return reply;
    }
    let caller = call.caller();
    if caller != ADMIN {
        return error("only admin can initialize");
    }
    if !storage::get_or("treasury", "")?.is_empty() {
        return error("contract is already initialized");
    }

    let treasury = match call.arg(0) {
        Ok(value) if is_valid_address(value) => value,
        _ => return error("invalid treasury address"),
    };

    must_set("treasury", treasury);
    must(events::emit_fields(
        "initialized",
        &[("treasury", treasury), ("admin", ADMIN), ("by", caller)],
    ));
    ok("Contract initialized")
}

pub fn register(call: &Call) -> Reply {
    let caller = call.caller();
    let name = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing name"),
    };
    let ext = match call.arg(1) {
        Ok(value) => value,
        Err(_) => return error("missing extension (.rtm)"),
    };
    let months: u64 = match call.arg(2).ok().and_then(|value| value.parse().ok()) {
        Some(value) => value,
        None => return error("invalid duration (3, 6, 12, 36)"),
    };

    if let Err(reason) = validate_name(name) {
        return error(&reason);
    }
    if ext != ".rtm" {
        return error("extension must be .rtm");
    }
    let total_price = match price_for_name(name, months) {
        Ok(price) => price,
        Err(reason) => return error(&reason),
    };
    let paid = call.value();
    if paid < total_price {
        return error(&format!(
            "insufficient payment: required {} atomic units, sent {}",
            total_price, paid
        ));
    }

    let full_domain = format!("{}{}", name, ext);
    let current_owner = storage::get_or(&owner_key(&full_domain), "")?;
    if !current_owner.is_empty() {
        let expires = read_expires(&full_domain)?;
        if expires == 0 {
            return error("domain state is invalid; contact support");
        }
        if now_ms() <= expires {
            return error("domain already taken and still active");
        }
        if in_grace(expires) {
            return error("domain is in its grace period; only the previous owner can renew it");
        }
    }

    let treasury = match treasury() {
        Ok(address) => address,
        Err(reason) => return error(&reason),
    };

    let expires_at = now_ms() + duration_ms(months);

    // From here on, any failure must trap so the whole call is rolled back.
    must_set(&owner_key(&full_domain), caller);
    must_set(&target_key(&full_domain), caller);
    must_set(&avatar_key(&full_domain), "");
    must_set(&expires_key(&full_domain), &expires_at.to_string());
    must_set(&sale_price_key(&full_domain), "");

    if !current_owner.is_empty() {
        must(index_remove_owner(&current_owner, &full_domain));
    }
    must(unlink_listing(&full_domain));
    must(index_add_owner(caller, &full_domain));

    must_pay(&treasury, total_price, "could not pay the treasury");
    must_pay(caller, paid - total_price, "could not refund the excess");

    must(events::emit_fields(
        "domain_registered",
        &[
            ("domain", &full_domain),
            ("owner", caller),
            ("price", &total_price.to_string()),
            ("expires_at", &expires_at.to_string()),
        ],
    ));

    ok(&format!(
        "Domain {} registered. Fee: {} atomic units, refunded: {} atomic units",
        full_domain,
        total_price,
        paid - total_price
    ))
}

pub fn renew(call: &Call) -> Reply {
    let caller = call.caller();
    let full_domain = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing domain"),
    };
    let months: u64 = match call.arg(1).ok().and_then(|value| value.parse().ok()) {
        Some(value) => value,
        None => return error("invalid duration (3, 6, 12, 36)"),
    };
    let name = match validate_domain(full_domain) {
        Ok(value) => value,
        Err(reason) => return error(&reason),
    };
    let total_price = match price_for_name(name, months) {
        Ok(price) => price,
        Err(reason) => return error(&reason),
    };

    let current_owner = storage::get_or(&owner_key(full_domain), "")?;
    if current_owner.is_empty() || current_owner != caller {
        return error("only the owner can renew this domain");
    }

    let expires = read_expires(full_domain)?;
    if expires == 0 {
        return error("domain state is invalid; contact support");
    }
    if now_ms() > expires.saturating_add(GRACE_MS) {
        return error("grace period has ended; the domain can be registered again");
    }

    let paid = call.value();
    if paid < total_price {
        return error(&format!(
            "insufficient payment: required {} atomic units, sent {}",
            total_price, paid
        ));
    }
    let treasury = match treasury() {
        Ok(address) => address,
        Err(reason) => return error(&reason),
    };

    let base = if now_ms() > expires { now_ms() } else { expires };
    let new_expires = base + duration_ms(months);

    must_set(&expires_key(full_domain), &new_expires.to_string());
    must_pay(&treasury, total_price, "could not pay the treasury");
    must_pay(caller, paid - total_price, "could not refund the excess");

    must(events::emit_fields(
        "domain_renewed",
        &[
            ("domain", full_domain),
            ("owner", caller),
            ("price", &total_price.to_string()),
            ("expires_at", &new_expires.to_string()),
        ],
    ));

    ok(&format!(
        "Domain {} renewed until {}. Fee: {} atomic units",
        full_domain, new_expires, total_price
    ))
}

pub fn update_records(call: &Call) -> Reply {
    if let Some(reply) = non_payable(call) {
        return reply;
    }
    let caller = call.caller();
    let full_domain = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing domain"),
    };
    let new_target = match call.arg(1) {
        Ok(value) => value,
        Err(_) => return error("missing target"),
    };
    let new_avatar = match call.arg(2) {
        Ok(value) => value,
        Err(_) => return error("missing avatar"),
    };

    let current_owner = storage::get_or(&owner_key(full_domain), "")?;
    if current_owner.is_empty() || current_owner != caller {
        return error("only the owner can update records");
    }
    let expires = read_expires(full_domain)?;
    if !is_active(expires) {
        return error("domain has expired");
    }
    if new_target.len() > MAX_TARGET_BYTES {
        return error(&format!(
            "target must be at most {MAX_TARGET_BYTES} bytes"
        ));
    }
    if new_avatar.len() > MAX_AVATAR_BYTES {
        return error(&format!(
            "avatar must be at most {MAX_AVATAR_BYTES} bytes"
        ));
    }
    if !is_valid_target(new_target) {
        return error("target must be a Retium address or an http, https, ipfs, ipns or ar URL");
    }

    must_set(&target_key(full_domain), new_target);
    must_set(&avatar_key(full_domain), new_avatar);
    must(events::emit_fields(
        "records_updated",
        &[
            ("domain", full_domain),
            ("target", new_target),
            ("avatar", new_avatar),
            ("by", caller),
        ],
    ));
    ok("Records updated")
}

pub fn list_for_sale(call: &Call) -> Reply {
    if let Some(reply) = non_payable(call) {
        return reply;
    }
    let caller = call.caller();
    let full_domain = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing domain"),
    };
    let price_str = match call.arg(1) {
        Ok(value) => value,
        Err(_) => return error("missing price"),
    };
    let price: u128 = match price_str.parse() {
        Ok(value) => value,
        Err(_) => return error("price must be an integer amount in atomic RTM"),
    };

    let current_owner = storage::get_or(&owner_key(full_domain), "")?;
    if current_owner.is_empty() || current_owner != caller {
        return error("only the current owner can list this domain");
    }

    // Price zero always means "remove the listing", even after expiry, so a
    // stale listing never hangs around forever.
    if price == 0 {
        must(unlink_listing(full_domain));
        must_set(&sale_price_key(full_domain), "");
        must(events::emit_fields(
            "domain_unlisted",
            &[("domain", full_domain), ("by", caller)],
        ));
        return ok(&format!("Domain {} removed from sale", full_domain));
    }

    let expires = read_expires(full_domain)?;
    if !is_active(expires) {
        return error("domain has expired; renew it before listing");
    }
    if remaining_ms(expires) < MIN_REMAINING_MS {
        return error("domain has less than 30 days left; renew it before listing");
    }

    must_set(&sale_price_key(full_domain), &price.to_string());
    if !must_is_linked(full_domain) {
        must(link_listing(full_domain));
    }
    must(events::emit_fields(
        "domain_listed",
        &[
            ("domain", full_domain),
            ("price", &price.to_string()),
            ("by", caller),
        ],
    ));
    ok(&format!(
        "Domain {} listed for {} atomic units",
        full_domain, price
    ))
}

pub fn buy_domain(call: &Call) -> Reply {
    let buyer = call.caller();
    let full_domain = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing domain"),
    };

    let current_owner = storage::get_or(&owner_key(full_domain), "")?;
    let sale_price_str = read_sale_price(full_domain)?;
    if current_owner.is_empty() || sale_price_str.is_empty() {
        return error("domain is not listed for sale");
    }
    if current_owner == buyer {
        return error("you cannot buy your own domain");
    }
    let sale_price: u128 = match sale_price_str.parse() {
        Ok(value) if value > 0 => value,
        _ => return error("listing price is invalid"),
    };

    let expires = read_expires(full_domain)?;
    if !is_active(expires) {
        return error("domain has expired and cannot be bought");
    }
    if remaining_ms(expires) < MIN_REMAINING_MS {
        return error("domain has less than 30 days left; the seller must renew it first");
    }

    let paid = call.value();
    if paid < sale_price {
        return error(&format!(
            "insufficient payment: required {} atomic units, sent {}",
            sale_price, paid
        ));
    }

    must_set(&owner_key(full_domain), buyer);
    must_set(&target_key(full_domain), buyer);
    must_set(&sale_price_key(full_domain), "");
    must(index_remove_owner(&current_owner, full_domain));
    must(index_add_owner(buyer, full_domain));
    must(unlink_listing(full_domain));

    must_pay(&current_owner, sale_price, "could not pay the seller");
    must_pay(buyer, paid - sale_price, "could not refund the excess");

    must(events::emit_fields(
        "domain_sold",
        &[
            ("domain", full_domain),
            ("buyer", buyer),
            ("seller", &current_owner),
            ("price", &sale_price.to_string()),
        ],
    ));

    ok(&format!(
        "Domain {} bought by {} for {} atomic units",
        full_domain, buyer, sale_price
    ))
}

pub fn transfer_domain(call: &Call) -> Reply {
    if let Some(reply) = non_payable(call) {
        return reply;
    }
    let caller = call.caller();
    let full_domain = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing domain"),
    };
    let recipient = match call.arg(1) {
        Ok(value) => value,
        Err(_) => return error("missing recipient"),
    };
    if !is_valid_address(recipient) {
        return error("recipient must be a valid Retium address");
    }
    if recipient == caller {
        return error("recipient is already the owner");
    }

    let current_owner = storage::get_or(&owner_key(full_domain), "")?;
    if current_owner.is_empty() || current_owner != caller {
        return error("you are not the owner");
    }
    let expires = read_expires(full_domain)?;
    if !is_active(expires) {
        return error("domain has expired; renew it before transferring");
    }

    must_set(&owner_key(full_domain), recipient);
    must_set(&target_key(full_domain), recipient);
    must_set(&sale_price_key(full_domain), "");
    must(index_remove_owner(caller, full_domain));
    must(index_add_owner(recipient, full_domain));
    must(unlink_listing(full_domain));

    must(events::emit_fields(
        "domain_transferred",
        &[("domain", full_domain), ("from", caller), ("to", recipient)],
    ));
    ok(&format!(
        "Domain {} transferred to {}",
        full_domain, recipient
    ))
}

pub fn set_treasury(call: &Call) -> Reply {
    if let Some(reply) = non_payable(call) {
        return reply;
    }
    let caller = call.caller();
    if caller != ADMIN {
        return error("only admin can change the treasury");
    }
    let new_treasury = match call.arg(0) {
        Ok(value) if is_valid_address(value) => value,
        _ => return error("invalid treasury address"),
    };
    let previous = storage::get_or("treasury", "")?;

    must_set("treasury", new_treasury);
    must(events::emit_fields(
        "treasury_updated",
        &[("old", &previous), ("new", new_treasury), ("by", caller)],
    ));
    ok("Treasury updated")
}

pub fn rescue(call: &Call) -> Reply {
    if let Some(reply) = non_payable(call) {
        return reply;
    }
    let caller = call.caller();
    if caller != ADMIN {
        return error("only admin can rescue funds");
    }
    let to = match call.arg(0) {
        Ok(value) if is_valid_address(value) => value,
        _ => return error("invalid recipient address"),
    };

    let self_id = match env::contract_id() {
        Ok(value) => value,
        Err(reason) => return error(&reason),
    };
    let balance = match call::rtm_balance_of(&self_id) {
        Ok(value) => value,
        Err(reason) => return error(&reason),
    };
    if balance == 0 {
        return ok("Nothing to rescue");
    }

    match call::send_rtm(to, balance) {
        Ok(reply) if !call::is_error_reply(&reply) => {}
        Ok(_) => return error("could not rescue the balance"),
        Err(reason) => return error(&format!("could not rescue the balance: {reason}")),
    }

    if let Err(reason) = events::emit_fields(
        "rescue",
        &[("to", to), ("amount", &balance.to_string()), ("by", caller)],
    ) {
        return error(&reason);
    }
    ok(&format!("Rescued {} atomic units to {}", balance, to))
}

// --- READ ACTIONS -----------------------------------------------------------

pub fn get_info(call: &Call) -> Reply {
    let full_domain = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing domain"),
    };

    let owner = storage::get_or(&owner_key(full_domain), "")?;
    if owner.is_empty() {
        return error("domain not found");
    }

    let target = storage::get_or(&target_key(full_domain), "")?;
    let avatar = storage::get_or(&avatar_key(full_domain), "")?;
    let sale_price = storage::get_or(&sale_price_key(full_domain), "")?;
    let expires_at = storage::get_or(&expires_key(full_domain), "")?;

    let expires_ms: u64 = expires_at.parse().unwrap_or(0);
    let expired = !is_active(expires_ms);
    let is_listed = !sale_price.is_empty();
    let price_display = if sale_price.is_empty() {
        "0".to_string()
    } else {
        sale_price
    };
    let expires_display = if expires_at.is_empty() {
        "0".to_string()
    } else {
        expires_at
    };

    ok(&format!(
        r#"{{"owner":{},"target":{},"avatar":{},"sale_price":{},"expires_at":{},"is_expired":{},"is_listed":{}}}"#,
        events::json_string(&owner),
        events::json_string(&target),
        events::json_string(&avatar),
        events::json_string(&price_display),
        events::json_string(&expires_display),
        expired,
        is_listed
    ))
}

pub fn get_domains_by_owner(call: &Call) -> Reply {
    let owner = match call.arg(0) {
        Ok(value) if !value.is_empty() => value,
        _ => return error("missing owner address"),
    };
    let after = call.arg(1).unwrap_or("");
    let limit = page_limit(call.arg(2).ok());

    let prefix = format!("owned:{}:", owner);
    let keys = storage::keys_with_prefix(&prefix)?;
    let mut domains = Vec::new();
    for key in keys {
        if storage::get_or(&key, "")?.is_empty() {
            continue;
        }
        if let Some(domain) = key.strip_prefix(&prefix) {
            if !domain.is_empty() {
                domains.push(domain.to_string());
            }
        }
    }
    domains.sort();

    let filtered: Vec<&String> = domains
        .iter()
        .filter(|domain| after.is_empty() || domain.as_str() > after)
        .collect();
    let page: Vec<String> = filtered
        .iter()
        .take(limit)
        .map(|domain| (*domain).clone())
        .collect();
    let next = if filtered.len() > limit {
        page.last().cloned().unwrap_or_default()
    } else {
        String::new()
    };

    ok(&format!(
        r#"{{"domains":{},"next":{}}}"#,
        json_array(&page),
        events::json_string(&next)
    ))
}

pub fn get_listed_domains(call: &Call) -> Reply {
    let after = call.arg(0).unwrap_or("");
    let limit = page_limit(call.arg(1).ok());

    let head = storage::get_or("listing_head", "")?;
    let mut cursor = if after.is_empty() {
        head
    } else {
        storage::get_or(&listing_next_key(after), "")?
    };

    let mut page = Vec::new();
    let mut steps = 0usize;
    while !cursor.is_empty() && page.len() < limit && steps < MAX_LIST_WALK {
        let next = storage::get_or(&listing_next_key(&cursor), "")?;
        let price = storage::get_or(&sale_price_key(&cursor), "")?;
        if !price.is_empty() {
            page.push(cursor.clone());
        }
        cursor = next;
        steps += 1;
    }

    let next = if !cursor.is_empty() && !page.is_empty() {
        page.last().cloned().unwrap_or_default()
    } else {
        String::new()
    };

    ok(&format!(
        r#"{{"domains":{},"next":{}}}"#,
        json_array(&page),
        events::json_string(&next)
    ))
}

pub fn quote(call: &Call) -> Reply {
    let name = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing name"),
    };
    let months: u64 = match call.arg(1).ok().and_then(|value| value.parse().ok()) {
        Some(value) => value,
        None => return error("invalid duration (3, 6, 12, 36)"),
    };
    if let Err(reason) = validate_name(name) {
        return error(&reason);
    }
    let price = match price_for_name(name, months) {
        Ok(value) => value,
        Err(reason) => return error(&reason),
    };
    ok(&format!(
        r#"{{"price":"{}","months":"{}"}}"#,
        price, months
    ))
}

pub fn is_reserved_action(call: &Call) -> Reply {
    let name = match call.arg(0) {
        Ok(value) => value,
        Err(_) => return error("missing name"),
    };
    ok(if is_reserved(name) { "true" } else { "false" })
}

pub fn get_config(_call: &Call) -> Reply {
    let treasury = storage::get_or("treasury", "")?;
    ok(&format!(
        r#"{{"treasury":{},"admin":{},"min_remaining_ms":"{}","grace_ms":"{}","min_name_length":"{}","max_name_length":"{}","version":{}}}"#,
        events::json_string(&treasury),
        events::json_string(ADMIN),
        MIN_REMAINING_MS,
        GRACE_MS,
        MIN_NAME_LEN,
        MAX_NAME_LEN,
        events::json_string(VERSION)
    ))
}

// --- TEST SUPPORT -----------------------------------------------------------

/// `is_linked` wrapped for call sites that must trap on a storage error.
fn must_is_linked(domain: &str) -> bool {
    match is_linked(domain) {
        Ok(value) => value,
        Err(reason) => fail(&reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retium_contract::test_host;

    const OTHER: &str = "rete2222222222222222222222222222222222222222222222222222222222222222222222";

    fn make_call(action: &str, caller: &str, args: &[&str], value: u128) -> Call {
        let args_json = args
            .iter()
            .map(|arg| format!("\"{}\"", arg))
            .collect::<Vec<_>>()
            .join(",");
        let raw = format!(
            r#"{{"action":"{}","caller":"{}","args":[{}],"value":"{}"}}"#,
            action, caller, args_json, value
        );
        Call::from_json(&raw).expect("test call must parse")
    }

    fn run(action: &str, caller: &str, args: &[&str], value: u128) -> Reply {
        handle(make_call(action, caller, args, value))
    }

    #[test]
    fn admin_is_a_valid_address() {
        assert!(is_valid_address(ADMIN), "ADMIN must be a 72 character rete address");
        assert_eq!(ADMIN.len(), 72);
    }

    #[test]
    fn init_is_locked_to_admin() {
        assert!(run("init", OTHER, &[ADMIN], 0).is_err());
        assert!(run("init", ADMIN, &[ADMIN], 0).is_ok());
    }

    #[test]
    fn init_rejects_value_and_bad_treasury() {
        assert!(run("init", ADMIN, &[ADMIN], 1).is_err());
        assert!(run("init", ADMIN, &["not-an-address"], 0).is_err());
        assert!(run("init", ADMIN, &[], 0).is_err());
    }

    #[test]
    fn address_validation_is_strict() {
        assert!(is_valid_address(ADMIN));
        assert!(!is_valid_address("rete"));
        assert!(!is_valid_address(&format!("{}g", &ADMIN[..71])));
        assert!(!is_valid_address(&format!("{}A", &ADMIN[..71])));
        assert!(!is_valid_address(&format!("retf{}", &ADMIN[4..])));
        assert!(!is_valid_address(&format!("{}0", ADMIN)));
    }

    #[test]
    fn name_rules_are_enforced() {
        assert!(validate_name("abc").is_ok());
        assert!(validate_name("a1-b2").is_ok());
        assert!(validate_name("ab").is_err());
        assert!(validate_name("abcdefghijk").is_err());
        assert!(validate_name("-abc").is_err());
        assert!(validate_name("abc-").is_err());
        assert!(validate_name("ab--cd").is_err());
        assert!(validate_name("ab-cd").is_ok());
        assert!(validate_name("abc!").is_err());
        assert!(validate_name("Abc").is_err());
        assert!(validate_name("admin").is_err());
        assert!(validate_name("retium").is_err());
    }

    #[test]
    fn reserved_names_are_listed() {
        assert!(is_reserved("admin"));
        assert!(is_reserved("rtm"));
        assert!(!is_reserved("alice"));
        assert_eq!(run("is_reserved", OTHER, &["admin"], 0).unwrap(), "{\"ok\":true,\"data\":\"true\"}");
        assert_eq!(run("is_reserved", OTHER, &["alice"], 0).unwrap(), "{\"ok\":true,\"data\":\"false\"}");
    }

    #[test]
    fn pricing_matches_the_published_table() {
        assert_eq!(price_for_name("abc", 12).unwrap(), 50 * ATOMIC_PER_RTM);
        assert_eq!(price_for_name("abcd", 12).unwrap(), 20 * ATOMIC_PER_RTM);
        assert_eq!(price_for_name("abcde", 12).unwrap(), 5 * ATOMIC_PER_RTM);
        assert_eq!(price_for_name("abc", 3).unwrap(), 1_250 * ATOMIC_PER_RTM / 100);
        assert_eq!(price_for_name("abc", 36).unwrap(), 12_750 * ATOMIC_PER_RTM / 100);
        assert!(price_for_name("abc", 9).is_err());
    }

    #[test]
    fn quote_returns_atomic_strings() {
        let reply = run("quote", OTHER, &["abc", "12"], 0).unwrap();
        assert!(reply.contains("50000000000000000000"), "{reply}");
        assert!(reply.contains("months"), "{reply}");
        assert!(reply.contains("12"), "{reply}");
        assert!(run("quote", OTHER, &["admin", "12"], 0).is_err());
        assert!(run("quote", OTHER, &["abc", "9"], 0).is_err());
    }

    #[test]
    fn register_validates_before_reading_state() {
        assert!(run("register", OTHER, &["ab", ".rtm", "12"], 0).is_err());
        assert!(run("register", OTHER, &["admin", ".rtm", "12"], 0).is_err());
        assert!(run("register", OTHER, &["ab--cd", ".rtm", "12"], 0).is_err());
        assert!(run("register", OTHER, &["abcdefghijk", ".rtm", "12"], 0).is_err());
        assert!(run("register", OTHER, &["alice", ".rtd", "12"], 0).is_err());
        assert!(run("register", OTHER, &["alice", ".rtm", "9"], 0).is_err());
        assert!(run("register", OTHER, &["alice", ".rtm", "12"], 0).is_err());
        assert!(run("register", OTHER, &["alice", ".rtm"], 0).is_err());
    }

    #[test]
    fn register_refuses_an_uninitialized_contract_even_when_paid() {
        // Enough for 5 RTM, but the treasury is not set off-chain.
        let err = run("register", OTHER, &["alice", ".rtm", "12"], 5 * ATOMIC_PER_RTM)
            .unwrap_err();
        assert!(err.contains("not initialized"), "{err}");
    }

    #[test]
    fn non_payable_actions_refuse_value() {
        assert!(run("update_records", OTHER, &["alice.rtm", "", ""], 1).is_err());
        assert!(run("list_for_sale", OTHER, &["alice.rtm", "1000"], 1).is_err());
        assert!(run("transfer_domain", OTHER, &["alice.rtm", ADMIN], 1).is_err());
        assert!(run("set_treasury", ADMIN, &[ADMIN], 1).is_err());
        assert!(run("rescue", ADMIN, &[ADMIN], 1).is_err());
    }

    #[test]
    fn transfer_checks_the_recipient_before_state() {
        assert!(run("transfer_domain", OTHER, &["alice.rtm", "bob"], 0).is_err());
        assert!(run("transfer_domain", OTHER, &["alice.rtm", OTHER], 0).is_err());
        // A valid recipient but a caller who owns nothing still fails.
        assert!(run("transfer_domain", OTHER, &["alice.rtm", ADMIN], 0).is_err());
    }

    #[test]
    fn list_and_buy_reject_bad_input() {
        assert!(run("list_for_sale", OTHER, &["alice.rtm", "not-a-number"], 0).is_err());
        assert!(run("buy_domain", OTHER, &["alice.rtm"], 0).is_err());
        assert!(run("update_records", OTHER, &["alice.rtm", "", ""], 0).is_err());
        assert!(run("renew", OTHER, &["alice.rtm", "12"], 0).is_err());
    }

    #[test]
    fn missing_domain_state_is_treated_as_expired() {
        assert!(!is_active(0));
        assert!(!in_grace(0));
        assert!(is_active(now_ms() + 1000));
    }

    #[test]
    fn targets_accept_addresses_and_known_schemes() {
        assert!(is_valid_target(""));
        assert!(is_valid_target(ADMIN));
        assert!(is_valid_target("https://example.com"));
        assert!(is_valid_target("ipfs://bafy"));
        assert!(!is_valid_target("javascript:alert(1)"));
        assert!(!is_valid_target("bob"));
    }

    #[test]
    fn pagination_limits_are_bounded() {
        assert_eq!(page_limit(None), DEFAULT_PAGE_SIZE);
        assert_eq!(page_limit(Some("0")), DEFAULT_PAGE_SIZE);
        assert_eq!(page_limit(Some("5")), 5);
        assert_eq!(page_limit(Some("1000")), MAX_PAGE_SIZE);
    }

    #[test]
    fn unknown_actions_are_rejected() {
        assert!(run("nope", OTHER, &[], 0).is_err());
    }

    #[test]
    fn config_is_readable_before_init() {
        let reply = run("get_config", OTHER, &[], 0).unwrap();
        assert!(reply.contains("v3.0.0"), "{reply}");
        assert!(reply.contains(ADMIN), "{reply}");
    }

    // --- TIME, GRACE AND PAYOUT PATHS --------------------------------------

    const NOW: u64 = 1_800_000_000_000;
    const NAME: &str = "alice.rtm";

    fn setup(owner: &str, expires: u64, sale_price: &str) {
        test_host::reset();
        test_host::set_time_ms(NOW as i64);
        storage::set("treasury", ADMIN).unwrap();
        storage::set(&owner_key(NAME), owner).unwrap();
        storage::set(&target_key(NAME), owner).unwrap();
        storage::set(&expires_key(NAME), &expires.to_string()).unwrap();
        storage::set(&sale_price_key(NAME), sale_price).unwrap();
        storage::set(&owned_key(owner, NAME), "1").unwrap();
        if !sale_price.is_empty() {
            storage::set("listing_head", NAME).unwrap();
            storage::set("listing_tail", NAME).unwrap();
        }
    }

    fn allow_rtm_transfers() {
        test_host::set_call_reply(call::RTM, "transfer", r#"{"ok":true}"#);
    }

    fn run_with_rollback(action: &str, caller: &str, args: &[&str], value: u128) -> Reply {
        let snapshot = test_host::snapshot();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(action, caller, args, value)
        })) {
            Ok(reply) => {
                if reply.is_err() {
                    test_host::restore(snapshot);
                }
                reply
            }
            Err(_) => {
                test_host::restore(snapshot);
                Err("contract trapped".to_string())
            }
        }
    }

    #[test]
    fn renewal_is_allowed_through_the_grace_boundary() {
        setup(OTHER, NOW - DAY_MS, "");
        allow_rtm_transfers();
        let price = price_for_name("alice", 3).unwrap();

        let reply = run_with_rollback("renew", OTHER, &[NAME, "3"], price).unwrap();
        assert!(reply.contains("renewed"), "{reply}");
        assert_eq!(
            read_expires(NAME).unwrap(),
            NOW + duration_ms(3),
            "a late renewal counts from now, not from the old expiry"
        );
        let calls = test_host::calls();
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0].args, [ADMIN.to_string(), price.to_string()]);
    }

    #[test]
    fn renewal_at_the_last_grace_millisecond_works_and_one_millisecond_later_does_not() {
        setup(OTHER, NOW - GRACE_MS, "");
        allow_rtm_transfers();
        let price = price_for_name("alice", 3).unwrap();
        assert!(run_with_rollback("renew", OTHER, &[NAME, "3"], price).is_ok());

        setup(OTHER, NOW - GRACE_MS - 1, "");
        allow_rtm_transfers();
        let err = run_with_rollback("renew", OTHER, &[NAME, "3"], price).unwrap_err();
        assert!(err.contains("grace period has ended"), "{err}");
    }

    #[test]
    fn registration_during_grace_is_reserved_for_the_previous_owner() {
        setup(OTHER, NOW - DAY_MS, "");
        allow_rtm_transfers();
        let price = price_for_name("alice", 12).unwrap();
        let err = run_with_rollback("register", ADMIN, &["alice", ".rtm", "12"], price)
            .unwrap_err();
        assert!(err.contains("grace period"), "{err}");
    }

    #[test]
    fn registration_works_one_millisecond_after_grace_ends() {
        setup(OTHER, NOW - GRACE_MS, "");
        allow_rtm_transfers();
        let price = price_for_name("alice", 12).unwrap();
        let err = run_with_rollback("register", ADMIN, &["alice", ".rtm", "12"], price)
            .unwrap_err();
        assert!(err.contains("grace period"), "{err}");

        test_host::set_time_ms((NOW + 1) as i64);
        allow_rtm_transfers();
        run_with_rollback("register", ADMIN, &["alice", ".rtm", "12"], price).unwrap();
        assert_eq!(storage::get_or(&owner_key(NAME), "").unwrap(), ADMIN);
        assert_eq!(
            read_expires(NAME).unwrap(),
            NOW + 1 + duration_ms(12),
            "the new registration starts at the current block time"
        );
        assert!(
            storage::get_or(&owned_key(OTHER, NAME), "").unwrap().is_empty(),
            "the old owner's index entry must be removed"
        );
        assert!(!storage::get_or(&owned_key(ADMIN, NAME), "").unwrap().is_empty());
    }

    #[test]
    fn listing_needs_a_full_thirty_days_of_life() {
        setup(OTHER, NOW + MIN_REMAINING_MS, "");
        run_with_rollback("list_for_sale", OTHER, &[NAME, "2000000000000000000"], 0).unwrap();
        assert_eq!(
            storage::get_or(&sale_price_key(NAME), "").unwrap(),
            "2000000000000000000"
        );
        assert_eq!(storage::get_or("listing_head", "").unwrap(), NAME);

        setup(OTHER, NOW + MIN_REMAINING_MS - 1, "");
        let err = run_with_rollback("list_for_sale", OTHER, &[NAME, "2000000000000000000"], 0)
            .unwrap_err();
        assert!(err.contains("less than 30 days"), "{err}");
        assert!(storage::get_or(&sale_price_key(NAME), "").unwrap().is_empty());
    }

    #[test]
    fn a_buy_needs_thirty_days_left_and_pays_the_seller_directly() {
        let sale: u128 = 2 * ATOMIC_PER_RTM;
        let paid = sale + ATOMIC_PER_RTM / 10;

        setup(OTHER, NOW + MIN_REMAINING_MS - 1, &sale.to_string());
        allow_rtm_transfers();
        let err = run_with_rollback("buy_domain", ADMIN, &[NAME], paid).unwrap_err();
        assert!(err.contains("less than 30 days"), "{err}");
        assert_eq!(
            storage::get_or(&owner_key(NAME), "").unwrap(),
            OTHER,
            "a refused buy must not move the name"
        );

        setup(OTHER, NOW + MIN_REMAINING_MS, &sale.to_string());
        allow_rtm_transfers();
        run_with_rollback("buy_domain", ADMIN, &[NAME], paid).unwrap();
        assert_eq!(storage::get_or(&owner_key(NAME), "").unwrap(), ADMIN);
        assert!(storage::get_or(&sale_price_key(NAME), "").unwrap().is_empty());
        assert!(storage::get_or("listing_head", "").unwrap().is_empty());

        let calls = test_host::calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0].method, "transfer");
        assert_eq!(
            calls[0].args,
            [OTHER.to_string(), sale.to_string()],
            "the seller is paid by the contract, not through the treasury"
        );
        assert_eq!(
            calls[1].args,
            [ADMIN.to_string(), (paid - sale).to_string()],
            "the excess goes back to the buyer"
        );
    }

    #[test]
    fn an_rtm_error_reply_traps_so_the_chain_undoes_the_sale() {
        let sale: u128 = 2 * ATOMIC_PER_RTM;
        setup(OTHER, NOW + MIN_REMAINING_MS, &sale.to_string());
        test_host::set_call_reply(
            call::RTM,
            "transfer",
            r#"{"ok":false,"error":"not enough RTM"}"#,
        );

        let err = run_with_rollback("buy_domain", ADMIN, &[NAME], sale).unwrap_err();
        assert!(err.contains("trapped"), "{err}");
        assert_eq!(
            storage::get_or(&owner_key(NAME), "").unwrap(),
            OTHER,
            "the rollback restores the seller as owner"
        );
        assert_eq!(
            storage::get_or(&sale_price_key(NAME), "").unwrap(),
            sale.to_string()
        );
        assert_eq!(storage::get_or("listing_head", "").unwrap(), NAME);
    }

    #[test]
    fn a_treasury_error_reply_traps_so_registration_does_not_stick() {
        test_host::reset();
        test_host::set_time_ms(NOW as i64);
        storage::set("treasury", ADMIN).unwrap();
        test_host::set_call_reply(
            call::RTM,
            "transfer",
            r#"{"ok":false,"error":"not enough RTM"}"#,
        );
        let price = price_for_name("alice", 12).unwrap();

        let err = run_with_rollback("register", OTHER, &["alice", ".rtm", "12"], price)
            .unwrap_err();
        assert!(err.contains("trapped"), "{err}");
        assert!(
            storage::get_or(&owner_key(NAME), "").unwrap().is_empty(),
            "a trapped registration must leave no owner behind"
        );
    }

    #[test]
    fn renewal_refunds_the_excess_to_the_owner() {
        setup(OTHER, NOW + 10 * DAY_MS, "");
        allow_rtm_transfers();
        let price = price_for_name("alice", 6).unwrap();
        let paid = price + 123;
        run_with_rollback("renew", OTHER, &[NAME, "6"], paid).unwrap();
        let calls = test_host::calls();
        assert_eq!(
            calls[0].args,
            [ADMIN.to_string(), price.to_string()]
        );
        assert_eq!(
            calls[1].args,
            [OTHER.to_string(), "123".to_string()]
        );
    }
}
