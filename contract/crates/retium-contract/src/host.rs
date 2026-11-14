//! The functions the Retium runtime gives a contract.
//!
//! These are the only things a contract can do that reach outside its own
//! memory. Everything else in this crate is a safe wrapper around them.
//!
//! On a non-wasm target (your unit tests, `cargo doc`) the imports are replaced
//! by stubs that report "no chain here", so a handler can be tested without a
//! node.

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    pub fn write_state(key_ptr: *const u8, key_len: i32, val_ptr: *const u8, val_len: i32);
    pub fn read_state(key_ptr: *const u8, key_len: i32, out_ptr: *mut u8, out_max_len: i32) -> i32;
    pub fn emit_event(
        type_ptr: *const u8,
        type_len: i32,
        data_ptr: *const u8,
        data_len: i32,
    ) -> i32;
    pub fn list_keys(
        prefix_ptr: *const u8,
        prefix_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32;
    pub fn get_self_contract_id(out_ptr: *mut u8, out_max_len: i32) -> i32;
    pub fn get_block_height() -> i64;
    pub fn get_block_time() -> i64;
    pub fn sha256(in_ptr: *const u8, in_len: i32, out_ptr: *mut u8) -> i32;
    pub fn sig_verify(
        msg_ptr: *const u8,
        msg_len: i32,
        sig_ptr: *const u8,
        sig_len: i32,
        pk_ptr: *const u8,
        pk_len: i32,
    ) -> i32;
    pub fn call_contract(
        id_ptr: *const u8,
        id_len: i32,
        method_ptr: *const u8,
        method_len: i32,
        args_ptr: *const u8,
        args_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32;
    pub fn call_contract_value(
        id_ptr: *const u8,
        id_len: i32,
        method_ptr: *const u8,
        method_len: i32,
        args_ptr: *const u8,
        args_len: i32,
        amount_ptr: *const u8,
        amount_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32;
    pub fn get_contract_code_hash(
        id_ptr: *const u8,
        id_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32;
    pub fn get_contract_version(
        id_ptr: *const u8,
        id_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32;
    pub fn set_contract_version(ver_ptr: *const u8, ver_len: i32) -> i32;
    pub fn supports_capability(
        id_ptr: *const u8,
        id_len: i32,
        cap_ptr: *const u8,
        cap_len: i32,
    ) -> i32;
    pub fn request_random(out_ptr: *mut u8, out_max_len: i32) -> i32;
}

// Off-chain stubs, so handlers can be unit-tested and documented.

/// A per-thread stand-in for the chain, for host unit tests.
///
/// Each test thread gets its own state, block time and call log, so tests that
/// run in parallel cannot see each other's writes. The real chain is never
/// involved and the wasm build does not include this module.
#[cfg(not(target_arch = "wasm32"))]
pub mod test_host {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// One cross-contract call made by the handler under test.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct TestCall {
        /// Contract that was called.
        pub contract: String,
        /// Method that was called.
        pub method: String,
        /// Arguments, already decoded from the chain's JSON array.
        pub args: Vec<String>,
        /// RTM attached to the call, in atomic units.
        pub value: String,
    }

    /// State and call log captured before a call, so a test can mimic rollback.
    #[derive(Clone, Debug)]
    pub struct Snapshot {
        state: BTreeMap<String, String>,
        calls: Vec<TestCall>,
    }

    thread_local! {
        static STATE: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
        static NOW_MS: RefCell<i64> = const { RefCell::new(0) };
        static REPLIES: RefCell<BTreeMap<(String, String), String>> =
            const { RefCell::new(BTreeMap::new()) };
        static CALLS: RefCell<Vec<TestCall>> = const { RefCell::new(Vec::new()) };
    }

    /// Clear state, block time, configured replies and the call log.
    pub fn reset() {
        STATE.with(|state| state.borrow_mut().clear());
        NOW_MS.with(|now| *now.borrow_mut() = 0);
        REPLIES.with(|replies| replies.borrow_mut().clear());
        CALLS.with(|calls| calls.borrow_mut().clear());
    }

    /// Set the block timestamp the handler sees, in milliseconds.
    pub fn set_time_ms(ms: i64) {
        NOW_MS.with(|now| *now.borrow_mut() = ms);
    }

    /// The block timestamp the handler sees.
    pub fn now_ms() -> i64 {
        NOW_MS.with(|now| *now.borrow())
    }

    /// Write a storage value exactly as the chain would. Empty clears the key.
    pub fn put(key: &str, value: &str) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            if value.is_empty() {
                state.remove(key);
            } else {
                state.insert(key.to_string(), value.to_string());
            }
        });
    }

    /// Read a storage value, or `None` when it is not set.
    pub fn get(key: &str) -> Option<String> {
        STATE.with(|state| state.borrow().get(key).cloned())
    }

    /// Keys that start with `prefix`, in order.
    pub fn keys_with_prefix(prefix: &str) -> Vec<String> {
        STATE.with(|state| {
            state
                .borrow()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect()
        })
    }

    /// Make calls to `contract.method` answer with `reply`.
    pub fn set_call_reply(contract: &str, method: &str, reply: &str) {
        REPLIES.with(|replies| {
            replies
                .borrow_mut()
                .insert((contract.to_string(), method.to_string()), reply.to_string());
        });
    }

    /// Cross-contract calls made since the last [`reset`].
    pub fn calls() -> Vec<TestCall> {
        CALLS.with(|calls| calls.borrow().clone())
    }

    /// Capture state and the call log.
    pub fn snapshot() -> Snapshot {
        Snapshot {
            state: STATE.with(|state| state.borrow().clone()),
            calls: CALLS.with(|calls| calls.borrow().clone()),
        }
    }

    /// Put state and the call log back as they were in `snapshot`.
    pub fn restore(snapshot: Snapshot) {
        STATE.with(|state| *state.borrow_mut() = snapshot.state);
        CALLS.with(|calls| *calls.borrow_mut() = snapshot.calls);
    }

    pub(crate) fn push_call(call: TestCall) {
        CALLS.with(|calls| calls.borrow_mut().push(call));
    }

    pub(crate) fn reply_for(contract: &str, method: &str) -> Option<String> {
        REPLIES.with(|replies| {
            replies
                .borrow()
                .get(&(contract.to_string(), method.to_string()))
                .cloned()
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod offchain {
    use alloc::string::{String, ToString};
    use core::slice;
    use core::str;

    use crate::host::test_host;
    use crate::host::test_host::TestCall;
    use crate::json::string_array;

    unsafe fn read_str(ptr: *const u8, len: i32) -> String {
        if ptr.is_null() || len <= 0 {
            return String::new();
        }
        let bytes = slice::from_raw_parts(ptr, len as usize);
        str::from_utf8(bytes).unwrap_or("").to_string()
    }

    unsafe fn write_out(out_ptr: *mut u8, out_max_len: i32, value: &str) -> i32 {
        if out_ptr.is_null() || out_max_len < 0 || value.len() > out_max_len as usize {
            return -1;
        }
        core::ptr::copy_nonoverlapping(value.as_ptr(), out_ptr, value.len());
        value.len() as i32
    }

    pub unsafe fn write_state(key_ptr: *const u8, key_len: i32, val_ptr: *const u8, val_len: i32) {
        let key = read_str(key_ptr, key_len);
        let value = read_str(val_ptr, val_len);
        test_host::put(&key, &value);
    }

    pub unsafe fn read_state(key_ptr: *const u8, key_len: i32, out_ptr: *mut u8, out_max_len: i32) -> i32 {
        let key = read_str(key_ptr, key_len);
        match test_host::get(&key) {
            Some(value) => write_out(out_ptr, out_max_len, &value),
            None => 0,
        }
    }

    pub unsafe fn emit_event(
        type_ptr: *const u8,
        type_len: i32,
        _data_ptr: *const u8,
        _data_len: i32,
    ) -> i32 {
        // Events are not asserted by these tests; accept the call so handlers
        // that emit after a write can finish normally.
        let _ = read_str(type_ptr, type_len);
        0
    }

    pub unsafe fn list_keys(
        prefix_ptr: *const u8,
        prefix_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32 {
        let prefix = read_str(prefix_ptr, prefix_len);
        let keys = test_host::keys_with_prefix(&prefix);
        let raw = alloc::format!(
            "[{}]",
            keys.iter()
                .map(|key| crate::events::json_string(key))
                .collect::<alloc::vec::Vec<_>>()
                .join(",")
        );
        write_out(out_ptr, out_max_len, &raw)
    }

    pub unsafe fn get_self_contract_id(_: *mut u8, _: i32) -> i32 {
        -1
    }

    pub unsafe fn get_block_height() -> i64 {
        0
    }

    pub unsafe fn get_block_time() -> i64 {
        test_host::now_ms()
    }
    pub unsafe fn sha256(_: *const u8, _: i32, _: *mut u8) -> i32 {
        -1
    }
    pub unsafe fn sig_verify(
        _: *const u8,
        _: i32,
        _: *const u8,
        _: i32,
        _: *const u8,
        _: i32,
    ) -> i32 {
        -1
    }
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn call_contract(
        id_ptr: *const u8,
        id_len: i32,
        method_ptr: *const u8,
        method_len: i32,
        args_ptr: *const u8,
        args_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32 {
        let contract = read_str(id_ptr, id_len);
        let method = read_str(method_ptr, method_len);
        let args = string_array(&read_str(args_ptr, args_len)).unwrap_or_default();
        test_host::push_call(TestCall {
            contract: contract.clone(),
            method: method.clone(),
            args,
            value: "0".to_string(),
        });
        match test_host::reply_for(&contract, &method) {
            Some(reply) => write_out(out_ptr, out_max_len, &reply),
            None => -1,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn call_contract_value(
        id_ptr: *const u8,
        id_len: i32,
        method_ptr: *const u8,
        method_len: i32,
        args_ptr: *const u8,
        args_len: i32,
        amount_ptr: *const u8,
        amount_len: i32,
        out_ptr: *mut u8,
        out_max_len: i32,
    ) -> i32 {
        let contract = read_str(id_ptr, id_len);
        let method = read_str(method_ptr, method_len);
        let args = string_array(&read_str(args_ptr, args_len)).unwrap_or_default();
        let value = read_str(amount_ptr, amount_len);
        test_host::push_call(TestCall {
            contract: contract.clone(),
            method: method.clone(),
            args,
            value,
        });
        match test_host::reply_for(&contract, &method) {
            Some(reply) => write_out(out_ptr, out_max_len, &reply),
            None => -1,
        }
    }
    pub unsafe fn get_contract_code_hash(_: *const u8, _: i32, _: *mut u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn get_contract_version(_: *const u8, _: i32, _: *mut u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn set_contract_version(_: *const u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn supports_capability(_: *const u8, _: i32, _: *const u8, _: i32) -> i32 {
        -1
    }
    pub unsafe fn request_random(_: *mut u8, _: i32) -> i32 {
        -1
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use offchain::*;
