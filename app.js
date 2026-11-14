// app.js - Retium Name Service Frontend Logic (Professional Web Wallet SDK)
'use strict';

// --- CONFIGURATION & STATE ---

const CONTRACT_ID = CONFIG.CONTRACT_ID;
const RPC_URL = CONFIG.RPC_URL.replace(/\/+$/, '');
const TX_API_URL = (CONFIG.EXPLORER_TX_URL || `${RPC_URL}/api/transaction`).replace(/\/+$/, '');

const MIN_NAME_LENGTH = 3;
const MAX_NAME_LENGTH = 10;
const NAME_PATTERN = /^[a-z0-9-]+$/;
// Mirrors RESERVED_NAMES in rns_v3. Names are stored without the extension.
const RESERVED_NAMES = new Set([
    'admin', 'retium', 'rtm', 'rtd', 'www', 'api', 'mail', 'support', 'help',
    'status', 'blog', 'docs', 'app', 'wallet', 'explorer', 'treasury',
    'official', 'system', 'root', 'null'
]);
const HISTORY_LIMIT = CONFIG.HISTORY_LIMIT || 200;
const DAY_MS = 24 * 60 * 60 * 1000;
const MONTH_MS = 30 * DAY_MS;
const GRACE_MS = 30 * DAY_MS;
const MIN_REMAINING_MS = 30 * DAY_MS;
const PAGE_SIZE = 12;
const TOAST_DURATION = 5000;
const ATOMIC_PER_RTM = 10n ** 18n;
const DB_VERSION = 3;
// rns_v3 only accepts a full 72 character Retium address.
const RECIPIENT_PATTERN = /^rete[0-9a-f]{68}$/;

const storedHistory = readJSON('rns_history', []);
let txHistory = Array.isArray(storedHistory) ? storedHistory : [];

let currentWallet = null;
let activeSellDomain = null;
let activeTransferDomain = null;
let activeRenewDomain = null;
let confirmResolve = null;
let quoteToken = 0;
let renewQuoteToken = 0;

const pageState = {
    mydomains: { cursor: '', loading: false, token: 0 },
    marketplace: { cursor: '', loading: false, token: 0 }
};

document.addEventListener('DOMContentLoaded', () => {
    initTabs();
    initWallet();
    initRegisterForm();
    initModals();
    initRenewModal();
    initConfirmModal();
    renderHistory();
});

// --- GENERAL HELPERS ---

function readJSON(key, fallback) {
    try {
        const raw = localStorage.getItem(key);
        return raw ? JSON.parse(raw) : fallback;
    } catch (err) {
        console.warn(`Failed to read "${key}" from localStorage.`, err);
        return fallback;
    }
}

function writeJSON(key, value) {
    try {
        localStorage.setItem(key, JSON.stringify(value));
    } catch (err) {
        console.warn(`Failed to save "${key}" to localStorage.`, err);
    }
}

function sameWallet(a, b) {
    return typeof a === 'string' && typeof b === 'string' && a.toLowerCase() === b.toLowerCase();
}

function shortAddress(address, head = 6, tail = 4) {
    const value = String(address || '');
    if (value.length <= head + tail) return value;
    return `${value.slice(0, head)}...${value.slice(-tail)}`;
}

function toAtomic(value) {
    const text = String(value ?? '').trim().replace(',', '.');
    const match = /^(?:(\d+)(?:\.(\d*))?|\.(\d+))$/.exec(text);
    if (!match) throw new Error(`Invalid amount: "${value}"`);

    const whole = match[1] !== undefined ? match[1] : '0';
    const fraction = match[1] !== undefined ? (match[2] || '') : match[3];
    if (fraction.length > 18) throw new Error('Maximum of 18 decimal places allowed.');

    return (BigInt(whole) * 10n ** 18n + BigInt((fraction + '0'.repeat(18)).slice(0, 18))).toString();
}

// Sanitize amount input: digits and one dot only, maximum 18 decimals.
function sanitizeAmountInput(raw) {
    const value = String(raw ?? '').replace(/,/g, '.').replace(/[^\d.]/g, '');
    const parts = value.split('.');
    if (parts.length === 1) return parts[0];
    return `${parts[0]}.${parts.slice(1).join('').slice(0, 18)}`;
}

function formatAmount(value) {
    const text = String(value ?? '').trim().replace(',', '.');
    if (!text.includes('.')) return text;
    return text.replace(/0+$/, '').replace(/\.$/, '') || '0';
}

function parseAmount(value) {
    const text = formatAmount(value);
    if (!/^\d*\.?\d+$/.test(text)) return null;
    const amount = Number(text);
    return Number.isFinite(amount) && amount > 0 ? amount : null;
}

function daysLeft(expiresAt) {
    const timestamp = Number(expiresAt);
    if (!Number.isFinite(timestamp)) return 0;
    return Math.max(0, Math.ceil((timestamp - Date.now()) / DAY_MS));
}

// 'active', 'grace' (expired but still renewable by the owner), 'expired'
// (available to anyone), or 'unknown' when the chain did not return an expiry.
function domainState(info) {
    const expires = Number(info && info.expiresAt);
    if (!expires || !Number.isFinite(expires)) return 'unknown';
    const now = Date.now();
    if (now <= expires) return 'active';
    if (now <= expires + GRACE_MS) return 'grace';
    return 'expired';
}

function remainingMs(info) {
    const expires = Number(info && info.expiresAt);
    if (!expires || !Number.isFinite(expires)) return 0;
    return Math.max(0, expires - Date.now());
}

function graceDaysLeft(info) {
    const expires = Number(info && info.expiresAt);
    if (!expires || !Number.isFinite(expires)) return 0;
    return Math.max(0, Math.ceil((expires + GRACE_MS - Date.now()) / DAY_MS));
}

// rns_v3 requires at least 30 days of life left before a listing or purchase.
function canListDomain(info) {
    return domainState(info) === 'active' && remainingMs(info) >= MIN_REMAINING_MS;
}

function canBuyDomain(info) {
    return domainState(info) === 'active' && remainingMs(info) >= MIN_REMAINING_MS;
}

function canRenewDomain(info) {
    const state = domainState(info);
    return state === 'active' || state === 'grace';
}

function canTransferDomain(info) {
    return domainState(info) === 'active';
}

// On-chain prices are atomic units (1 RTM = 10^18) and must stay exact.
function fromAtomic(value) {
    const text = String(value ?? '').trim();
    if (!/^\d+$/.test(text)) return '0';

    const atomic = BigInt(text);
    const whole = atomic / ATOMIC_PER_RTM;
    const fraction = (atomic % ATOMIC_PER_RTM)
        .toString()
        .padStart(18, '0')
        .replace(/0+$/, '');

    return fraction ? `${whole}.${fraction}` : whole.toString();
}

function normalizeAtomicPrice(value) {
    const text = String(value ?? '').trim();
    if (!/^\d+$/.test(text)) return null;
    return BigInt(text) > 0n ? text : null;
}

function extractAtomicField(raw, field) {
    const match = new RegExp(`"${field}"\\s*:\\s*(\\d+)`).exec(String(raw || ''));
    return match ? match[1] : null;
}

// Read a contract method and unwrap the RPC and contract reply envelopes.
async function queryContract(method, args = []) {
    // The RPC splits `args` on literal commas, so encode each value but keep
    // the separators raw. URLSearchParams would turn them into %2C and the
    // node would treat the whole string as one argument.
    const params = [`method=${encodeURIComponent(method)}`];
    if (args.length > 0) {
        params.push(`args=${args.map(value => encodeURIComponent(String(value))).join(',')}`);
    }

    const res = await fetch(`${RPC_URL}/api/contract/${CONTRACT_ID}/query?${params.join('&')}`);
    if (!res.ok) throw new Error(`RPC returned HTTP ${res.status}`);

    const raw = await res.text();
    let payload;
    try { payload = JSON.parse(raw); } catch (_) { payload = raw; }

    let node = payload;
    if (typeof node === 'string') {
        try { node = JSON.parse(node); } catch (_) { /* keep the raw string */ }
    }
    if (node && typeof node.output === 'string') {
        try { node = JSON.parse(node.output); } catch (_) { /* keep the raw output */ }
    }

    if (node && node.ok === false) {
        const error = String(node.error || node.message || 'Contract query failed');
        const notFound = /not found|not exist|no domain/i.test(error);
        return { exists: false, error, notFound, unknown: !notFound };
    }

    let data = node && Object.prototype.hasOwnProperty.call(node, 'data') ? node.data : node;

    if (typeof data === 'string' && data.trim()) {
        // JSON.parse turns atomic integers into doubles. Keep the exact digits.
        const salePrice = extractAtomicField(data, 'sale_price');
        const expiresAt = extractAtomicField(data, 'expires_at');

        try {
            const parsed = JSON.parse(data);
            if (parsed && typeof parsed === 'object') {
                if (salePrice !== null) parsed.sale_price = salePrice;
                if (expiresAt !== null) parsed.expires_at = expiresAt;
            }
            data = parsed;
        } catch (_) {
            // Plain string replies (owner and listing indexes) stay strings.
        }
    }

    return { exists: true, data };
}

// Normalize a get_info reply so the UI always sees one shape.
function normalizeDomainInfo(domain, data) {
    if (!data || typeof data !== 'object') return null;
    const owner = data.owner ? String(data.owner) : '';
    if (!owner) return null;

    const expiresAt = data.expires_at != null ? String(data.expires_at) : '';
    return {
        domain,
        owner,
        target: data.target ? String(data.target) : '',
        avatar: data.avatar ? String(data.avatar) : '',
        salePrice: normalizeAtomicPrice(data.sale_price),
        expiresAt: expiresAt === '0' ? '' : expiresAt,
        isExpired: Boolean(data.is_expired),
        isListed: Boolean(data.is_listed)
    };
}

async function fetchDomainInfo(domain) {
    try {
        const result = await queryContract('get_info', [domain]);
        if (!result.exists || !result.data) return null;
        return normalizeDomainInfo(domain, result.data);
    } catch (err) {
        console.warn(`Could not load ${domain} from the contract.`, err);
        return null;
    }
}

// Like fetchDomainInfo, but keeps "missing" and "could not ask" apart so the
// register form does not tell a user a DOMAIN IS AVAILABLE when the RPC failed.
async function lookupDomain(domain) {
    try {
        const result = await queryContract('get_info', [domain]);
        if (!result.exists) {
            return { info: null, unknown: !result.notFound };
        }
        const info = normalizeDomainInfo(domain, result.data);
        return info ? { info, unknown: false } : { info: null, unknown: true };
    } catch (err) {
        console.warn(`Could not look up ${domain}.`, err);
        return { info: null, unknown: true };
    }
}

// Read one page of a v3 index. Returns { domains, next } or null on failure.
async function fetchDomainPage(method, args = []) {
    try {
        const result = await queryContract(method, args);
        if (!result.exists) return null;

        const data = result.data;
        if (data && typeof data === 'object' && !Array.isArray(data)) {
            const domains = Array.isArray(data.domains)
                ? data.domains.map(String).map(name => name.trim()).filter(Boolean)
                : [];
            const next = data.next ? String(data.next) : '';
            return { domains, next };
        }

        // Tolerate a v2-style array or comma separated index during rollout.
        if (Array.isArray(data)) {
            return { domains: data.map(String).map(name => name.trim()).filter(Boolean), next: '' };
        }
        if (typeof data === 'string') {
            return { domains: data.split(',').map(name => name.trim()).filter(Boolean), next: '' };
        }
        return { domains: [], next: '' };
    } catch (err) {
        console.warn(`Could not read ${method} from the contract.`, err);
        return null;
    }
}

function makeButton(label, className, onClick, styles) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = className;
    button.textContent = label;
    if (styles) Object.assign(button.style, styles);
    button.addEventListener('click', onClick);
    return button;
}

function emptyMessage(container, text) {
    const p = document.createElement('p');
    p.className = 'empty-msg';
    p.textContent = text;
    container.appendChild(p);
}

// --- THEMED NOTIFICATIONS ---

function notify(message, type = 'info') {
    const container = document.getElementById('toastContainer');
    if (!container) {
        console.log(`[${type}] ${message}`);
        return;
    }

    while (container.children.length >= 4) {
        container.firstElementChild.remove();
    }

    const toast = document.createElement('div');
    toast.className = `toast is-${type}`;
    toast.setAttribute('role', type === 'error' ? 'alert' : 'status');
    toast.textContent = message;
    container.appendChild(toast);

    requestAnimationFrame(() => toast.classList.add('is-visible'));

    const duration = type === 'error' ? TOAST_DURATION + 3000 : TOAST_DURATION;
    setTimeout(() => {
        toast.classList.remove('is-visible');
        setTimeout(() => toast.remove(), 250);
    }, duration);
}

function confirmAction(options = {}) {
    const modal = document.getElementById('confirmModal');
    if (!modal) return Promise.resolve(window.confirm(options.message || options.title || 'Confirm?'));

    document.getElementById('confirmTitle').textContent = options.title || 'Confirm';
    document.getElementById('confirmMessage').textContent = options.message || '';

    const okBtn = document.getElementById('confirmOkBtn');
    okBtn.textContent = options.confirmLabel || 'Confirm';
    okBtn.className = options.danger ? 'btn-danger' : 'btn-primary';

    if (confirmResolve) {
        const previous = confirmResolve;
        confirmResolve = null;
        previous(false);
    }

    modal.style.display = 'flex';
    return new Promise(resolve => { confirmResolve = resolve; });
}

function settleConfirm(result) {
    const modal = document.getElementById('confirmModal');
    if (modal) modal.style.display = 'none';

    if (confirmResolve) {
        const resolve = confirmResolve;
        confirmResolve = null;
        resolve(result);
    }
}

function initConfirmModal() {
    const modal = document.getElementById('confirmModal');
    const okBtn = document.getElementById('confirmOkBtn');
    const cancelBtn = document.getElementById('confirmCancelBtn');
    if (!modal || !okBtn || !cancelBtn) return;

    okBtn.addEventListener('click', () => settleConfirm(true));
    cancelBtn.addEventListener('click', () => settleConfirm(false));
    modal.addEventListener('click', (event) => {
        if (event.target === modal) settleConfirm(false);
    });
    document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape' && modal.style.display !== 'none') settleConfirm(false);
    });
}

// --- UI LOGIC ---

function initTabs() {
    const tabBtns = document.querySelectorAll('.tab-btn');
    const tabPanes = document.querySelectorAll('.tab-pane');

    tabBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            tabBtns.forEach(b => b.classList.remove('active'));
            tabPanes.forEach(p => p.classList.remove('active'));
            btn.classList.add('active');

            const target = btn.getAttribute('data-tab');
            const pane = document.getElementById(target);
            if (pane) pane.classList.add('active');

            if (target === 'mydomains') loadMyDomains();
            if (target === 'marketplace') loadMarketplace();
            if (target === 'history') renderHistory();
        });
    });
}

async function initWallet() {
    const connectBtn = document.getElementById('connectBtn');
    const walletBadge = document.getElementById('walletAddress');
    const disconnectBtn = document.getElementById('disconnectBtn');

    connectBtn.addEventListener('click', async () => {
        try {
            connectBtn.disabled = true;
            connectBtn.textContent = 'Connecting...';

            const address = await requestAccounts();
            currentWallet = address;

            walletBadge.textContent = shortAddress(address);
            walletBadge.title = address;
            connectBtn.textContent = 'Connected';
            connectBtn.disabled = true;
            disconnectBtn.hidden = false;

            loadMyDomains();
            loadMarketplace();
            renderHistory();
        } catch (err) {
            connectBtn.disabled = false;
            connectBtn.textContent = 'Connect Wallet';

            if (err && err.code === 4001) return; // User rejected the request
            notify('Failed to connect wallet: ' + (err.message || err), 'error');
        }
    });

    disconnectBtn.addEventListener('click', () => {
        currentWallet = null;

        walletBadge.textContent = 'Not Connected';
        walletBadge.removeAttribute('title');
        connectBtn.textContent = 'Connect Wallet';
        connectBtn.disabled = false;
        disconnectBtn.hidden = true;

        loadMyDomains();
        loadMarketplace();
        renderHistory();
        notify('Wallet disconnected.', 'info');
    });
}

async function waitForRetium(timeoutMs = 3000) {
    if (window.retium) return window.retium;

    return new Promise((resolve) => {
        let settled = false;
        const onReady = () => finish(window.retium ?? null);
        const finish = (value) => {
            if (settled) return;
            settled = true;
            clearTimeout(timer);
            window.removeEventListener('retium#initialized', onReady);
            resolve(value);
        };
        const timer = setTimeout(() => finish(window.retium ?? null), timeoutMs);
        window.addEventListener('retium#initialized', onReady, { once: true });
    });
}

async function requestAccounts() {
    const retium = await waitForRetium();
    if (!retium) throw new Error('Retium Web Wallet not found. Make sure the wallet extension is installed.');

    const result = await retium.request({ method: 'retium_requestAccounts' });
    const address = Array.isArray(result) ? result[0] : (result && (result.address || (result.accounts && result.accounts[0])));

    if (!address) throw new Error('The wallet did not return an address.');
    return address;
}

async function waitForFinal(txId, options = {}) {
    const intervalMs = options.intervalMs || 2000;
    const timeoutMs = options.timeoutMs || 180000;
    const startedAt = Date.now();

    while (Date.now() - startedAt < timeoutMs) {
        await new Promise(resolve => setTimeout(resolve, intervalMs));

        let tx;
        try {
            const res = await fetch(`${TX_API_URL}/${encodeURIComponent(txId)}`);
            if (!res.ok) continue;
            tx = await res.json();
        } catch (err) {
            console.warn('Waiting for block...', err.message);
            continue;
        }

        const status = String(tx.status || '').toLowerCase();

        if (status === 'hardfinal') {
            if (String(tx.exec_status || '').toLowerCase() === 'failed') {
                throw new Error(tx.exec_error || 'Contract execution failed.');
            }
            return tx;
        }

        if (status === 'rejected' || status === 'voided') {
            throw new Error(tx.exec_error || 'The transaction was rejected by the network.');
        }
    }

    throw new Error(`Transaction was not finalized after ${Math.round(timeoutMs / 1000)} seconds. Check the explorer for TX: ${txId}`);
}

function initRegisterForm() {
    const nameInput = document.getElementById('regName');
    const extSelect = document.getElementById('regExt');
    const durationSelect = document.getElementById('regDuration');
    const costPreview = document.getElementById('costPreview');
    const registerBtn = document.getElementById('registerBtn');
    const formGroup = nameInput.parentElement.parentElement;

    // Domain availability status element
    const statusText = document.createElement('p');
    statusText.id = 'domainStatus';
    statusText.className = 'form-status';
    formGroup.appendChild(statusText);

    let checkToken = 0;
    let debounceTimer = null;
    let submitting = false;

    function setStatus(message, state = '') {
        statusText.textContent = message;
        statusText.className = state ? `form-status is-${state}` : 'form-status';
    }

    function setRegisterEnabled(enabled) {
        registerBtn.disabled = !enabled;
        registerBtn.style.opacity = enabled ? '1' : '0.5';
    }

    function normalizeName() {
        const cleaned = nameInput.value.toLowerCase().replace(/[^a-z0-9-]/g, '').slice(0, MAX_NAME_LENGTH);
        if (nameInput.value !== cleaned) nameInput.value = cleaned;
        return cleaned;
    }

    // Mirrors validate_name in rns_v3 so the user gets feedback before a
    // wallet popup; the contract remains the final authority.
    function validateName(name) {
        if (!name) return { ok: false, message: '' };
        if (name.length < MIN_NAME_LENGTH) return { ok: false, message: `Name must be at least ${MIN_NAME_LENGTH} characters.` };
        if (name.length > MAX_NAME_LENGTH) return { ok: false, message: `Name must be at most ${MAX_NAME_LENGTH} characters.` };
        if (!NAME_PATTERN.test(name)) return { ok: false, message: 'Only lowercase letters, numbers, and hyphens (-) are allowed.' };
        if (name.startsWith('-') || name.endsWith('-')) {
            return { ok: false, message: 'Name cannot start or end with a hyphen.' };
        }
        if (name.length >= 4 && name[2] === '-' && name[3] === '-') {
            return { ok: false, message: "Name cannot contain '--' at positions 3 and 4." };
        }
        if (RESERVED_NAMES.has(name)) {
            return { ok: false, message: 'This name is reserved.' };
        }
        return { ok: true, message: '' };
    }

    // Ask the contract for the exact atomic price. This replaces the old local
    // price table so the UI can never disagree with what rns_v3 charges.
    async function refreshQuote() {
        const token = ++quoteToken;
        const name = normalizeName();
        const duration = String(parseInt(durationSelect.value, 10));
        const validation = validateName(name);

        if (!name || !validation.ok) {
            costPreview.textContent = '0';
            return { price: null, validation };
        }

        try {
            const result = await queryContract('quote', [name, duration]);
            if (token !== quoteToken) return { price: null, validation };
            const price = result.exists && result.data && result.data.price
                ? String(result.data.price)
                : null;
            costPreview.textContent = price ? fromAtomic(price) : '—';
            return { price, validation };
        } catch (err) {
            if (token === quoteToken) costPreview.textContent = '—';
            return { price: null, validation };
        }
    }

    async function checkDomainStatus() {
        const token = ++checkToken;
        const name = normalizeName();
        const ext = extSelect.value;
        const validation = validateName(name);

        if (!validation.ok) {
            if (!name) setStatus('');
            else setStatus(validation.message, 'warning');
            setRegisterEnabled(false);
            return;
        }

        setStatus('Checking availability...', 'checking');
        setRegisterEnabled(false);

        try {
            const lookup = await lookupDomain(name + ext);
            if (token !== checkToken) return; // Ignore stale response

            if (!lookup.unknown && !lookup.info) {
                setStatus('Domain is available!', 'success');
                setRegisterEnabled(true);
                return;
            }

            if (lookup.unknown) {
                setStatus('Availability could not be confirmed. You can still try to register.', 'warning');
                setRegisterEnabled(true);
                return;
            }

            const info = lookup.info;
            const state = domainState(info);
            const own = currentWallet && sameWallet(info.owner, currentWallet);

            if (state === 'active') {
                setStatus(own ? 'You already own this domain.' : 'Domain is already taken.', own ? 'info' : 'error');
                setRegisterEnabled(false);
            } else if (state === 'grace') {
                setStatus(
                    own
                        ? 'Domain has expired. Renew it from My Domains before the grace period ends.'
                        : 'Domain is in its grace period. Only the previous owner can renew it.',
                    'warning'
                );
                setRegisterEnabled(false);
            } else {
                setStatus('Domain is available!', 'success');
                setRegisterEnabled(true);
            }
        } catch (err) {
            if (token !== checkToken) return;
            console.warn('Domain check failed:', err.message);
            setStatus('Could not check availability (RPC). You can still try to register.', 'warning');
            setRegisterEnabled(true);
        }
    }

    nameInput.addEventListener('input', () => {
        normalizeName();
        refreshQuote();
        clearTimeout(debounceTimer);

        // Prevent Register clicks while the check is still pending.
        setRegisterEnabled(false);
        if (nameInput.value.trim()) setStatus('Checking availability...', 'checking');
        else setStatus('');

        debounceTimer = setTimeout(checkDomainStatus, 350);
    });

    extSelect.addEventListener('change', () => {
        refreshQuote();
        checkDomainStatus();
    });

    durationSelect.addEventListener('change', () => {
        refreshQuote();
    });

    registerBtn.addEventListener('click', async () => {
        if (submitting) return;

        const name = normalizeName();
        const ext = extSelect.value;
        const duration = durationSelect.value;
        const fullDomain = name + ext;
        const validation = validateName(name);

        if (!currentWallet) { notify('Connect your wallet first.', 'warning'); return; }
        if (!validation.ok) { notify(validation.message || 'Invalid domain name.', 'error'); return; }

        const quote = await refreshQuote();
        if (!quote.price) {
            notify('Could not read the registration price from the contract. Please try again.', 'error');
            return;
        }
        const price = quote.price;

        try {
            submitting = true;
            registerBtn.textContent = 'Processing...';
            setRegisterEnabled(false);

            const retium = await waitForRetium();
            if (!retium) throw new Error('Retium Web Wallet not found.');

            const { txId } = await retium.request({
                method: 'retium_call',
                params: {
                    contractId: CONTRACT_ID,
                    method: 'register',
                    args: [name, ext, String(duration)],
                    amountAtomic: price
                }
            });

            notify(`Transaction submitted. TX: ${txId}\nWaiting for finalization...`, 'info');
            await waitForFinal(txId);

            addHistory('Register', fullDomain, `${fromAtomic(price)} RTM (${duration} months)`, 'Success');
            saveOwnedDomain(fullDomain, currentWallet, parseInt(duration, 10));
            notify('Domain registered successfully!', 'success');

            nameInput.value = '';
            costPreview.textContent = '0';
            setStatus('');
            loadMyDomains();
            loadMarketplace();
        } catch (err) {
            notify('Registration failed: ' + (err.message || err), 'error');
            await checkDomainStatus();
        } finally {
            submitting = false;
            registerBtn.textContent = 'Register Domain';
            // Re-enable the button only when the name is valid and available.
            if (!nameInput.value.trim()) setRegisterEnabled(false);
        }
    });
}

function initModals() {
    const sellModal = document.getElementById('sellModal');
    const cancelSellBtn = document.getElementById('cancelSellBtn');
    const confirmSellBtn = document.getElementById('confirmSellBtn');
    const sellPriceInput = document.getElementById('sellPriceInput');

    const transferModal = document.getElementById('transferModal');
    const cancelTransferBtn = document.getElementById('cancelTransferBtn');
    const confirmTransferBtn = document.getElementById('confirmTransferBtn');
    const transferRecipientInput = document.getElementById('transferRecipientInput');

    function openModal(modal) {
        modal.style.display = 'flex';
    }

    function closeModal(modal) {
        modal.style.display = 'none';
    }

    sellPriceInput.addEventListener('input', () => {
        const cleaned = sanitizeAmountInput(sellPriceInput.value);
        if (sellPriceInput.value !== cleaned) sellPriceInput.value = cleaned;
    });

    cancelSellBtn.addEventListener('click', () => {
        closeModal(sellModal);
        sellPriceInput.value = '';
        activeSellDomain = null;
    });

    confirmSellBtn.addEventListener('click', async () => {
        const domain = activeSellDomain;
        const price = sanitizeAmountInput(sellPriceInput.value);
        const amount = parseAmount(price);

        if (!domain) { notify('No domain selected.', 'error'); return; }
        if (!currentWallet) { notify('Connect your wallet first.', 'warning'); return; }
        if (amount === null) { notify('Enter a valid price greater than 0.', 'error'); return; }

        const info = await fetchDomainInfo(domain);
        if (!info) { notify('Could not read this domain from the contract.', 'error'); return; }
        if (!sameWallet(info.owner, currentWallet)) { notify('Only the owner can list this domain.', 'error'); return; }
        if (!canListDomain(info)) {
            notify('This domain needs at least 30 days of life left. Renew it before listing.', 'error');
            loadMyDomains();
            return;
        }

        try {
            confirmSellBtn.disabled = true;
            confirmSellBtn.textContent = 'Processing...';

            const retium = await waitForRetium();
            if (!retium) throw new Error('Retium Web Wallet not found.');

            const { txId } = await retium.request({
                method: 'retium_call',
                params: {
                    contractId: CONTRACT_ID,
                    method: 'list_for_sale',
                    args: [domain, toAtomic(price)],
                    amountAtomic: '0'
                }
            });

            notify(`Listing transaction submitted. TX: ${txId}`, 'info');
            await waitForFinal(txId);

            notify(`Domain ${domain} is now listed for ${formatAmount(price)} RTM.`, 'success');
            listDomainForSaleStorage(domain, toAtomic(price));
            closeModal(sellModal);
            sellPriceInput.value = '';
            activeSellDomain = null;
            loadMyDomains();
            loadMarketplace();
        } catch (err) {
            notify('Failed to list domain: ' + (err.message || err), 'error');
        } finally {
            confirmSellBtn.disabled = false;
            confirmSellBtn.textContent = 'Confirm Listing';
        }
    });

    cancelTransferBtn.addEventListener('click', () => {
        closeModal(transferModal);
        transferRecipientInput.value = '';
        activeTransferDomain = null;
    });

    confirmTransferBtn.addEventListener('click', async () => {
        const domain = activeTransferDomain;
        const recipient = transferRecipientInput.value.trim();

        if (!domain) { notify('No domain selected.', 'error'); return; }
        if (!currentWallet) { notify('Connect your wallet first.', 'warning'); return; }
        if (!recipient) { notify('Enter the recipient address.', 'error'); return; }
        if (!RECIPIENT_PATTERN.test(recipient)) {
            notify('Enter a valid Retium address: rete followed by 68 lowercase hex characters.', 'error');
            return;
        }
        if (sameWallet(recipient, currentWallet)) { notify('You cannot transfer a domain to your own wallet.', 'warning'); return; }

        const info = await fetchDomainInfo(domain);
        if (!info) { notify('Could not read this domain from the contract.', 'error'); return; }
        if (!canTransferDomain(info)) {
            notify('This domain has expired. Renew it before transferring.', 'error');
            loadMyDomains();
            return;
        }

        try {
            confirmTransferBtn.disabled = true;
            confirmTransferBtn.textContent = 'Processing...';

            const retium = await waitForRetium();
            if (!retium) throw new Error('Retium Web Wallet not found.');

            const { txId } = await retium.request({
                method: 'retium_call',
                params: {
                    contractId: CONTRACT_ID,
                    method: 'transfer_domain',
                    args: [domain, recipient],
                    amountAtomic: '0'
                }
            });

            notify(`Transfer transaction submitted. TX: ${txId}`, 'info');
            await waitForFinal(txId);

            notify(`Domain transferred to ${recipient}.`, 'success');
            transferDomainStorage(domain, recipient);
            closeModal(transferModal);
            transferRecipientInput.value = '';
            activeTransferDomain = null;
            loadMyDomains();
            loadMarketplace();
        } catch (err) {
            notify('Transfer failed: ' + (err.message || err), 'error');
        } finally {
            confirmTransferBtn.disabled = false;
            confirmTransferBtn.textContent = 'Send Domain';
        }
    });

    // Close modals via overlay click or the Escape key
    [[sellModal, sellPriceInput, () => { activeSellDomain = null; }],
     [transferModal, transferRecipientInput, () => { activeTransferDomain = null; }]]
        .forEach(([modal, input, reset]) => {
            modal.addEventListener('click', (event) => {
                if (event.target !== modal) return;
                closeModal(modal);
                input.value = '';
                reset();
            });
        });

    document.addEventListener('keydown', (event) => {
        if (event.key !== 'Escape') return;

        const confirmModal = document.getElementById('confirmModal');
        if (confirmModal && confirmModal.style.display !== 'none') return;

        if (sellModal.style.display !== 'none') {
            closeModal(sellModal);
            sellPriceInput.value = '';
            activeSellDomain = null;
        }
        if (transferModal.style.display !== 'none') {
            closeModal(transferModal);
            transferRecipientInput.value = '';
            activeTransferDomain = null;
        }
    });

    // Exposed so card buttons can open the modals
    window.openSellModal = (domain) => {
        activeSellDomain = domain;
        document.getElementById('modalDomainLabel').textContent = `Domain: ${domain}`;
        openModal(sellModal);
        sellPriceInput.focus();
    };

    window.openTransferModal = (domain) => {
        activeTransferDomain = domain;
        document.getElementById('transferDomainLabel').textContent = `Domain: ${domain}`;
        openModal(transferModal);
        transferRecipientInput.focus();
    };
}

// --- RENEW MODAL ---

function initRenewModal() {
    const modal = document.getElementById('renewModal');
    if (!modal) return;

    const label = document.getElementById('renewDomainLabel');
    const durationSelect = document.getElementById('renewDuration');
    const pricePreview = document.getElementById('renewPricePreview');
    const cancelBtn = document.getElementById('cancelRenewBtn');
    const confirmBtn = document.getElementById('confirmRenewBtn');

    let submitting = false;

    function reset() {
        activeRenewDomain = null;
        pricePreview.textContent = '0';
    }

    async function previewPrice() {
        const domain = activeRenewDomain;
        if (!domain) { pricePreview.textContent = '0'; return null; }

        const token = ++renewQuoteToken;
        const name = domain.replace(/\.rtm$/, '');
        const months = String(parseInt(durationSelect.value, 10));

        try {
            const result = await queryContract('quote', [name, months]);
            if (token !== renewQuoteToken) return null;
            if (!result.exists || !result.data || !result.data.price) {
                pricePreview.textContent = '—';
                return null;
            }
            const price = String(result.data.price);
            pricePreview.textContent = fromAtomic(price);
            return price;
        } catch (err) {
            if (token === renewQuoteToken) pricePreview.textContent = '—';
            return null;
        }
    }

    durationSelect.addEventListener('change', previewPrice);

    cancelBtn.addEventListener('click', () => {
        modal.style.display = 'none';
        reset();
    });

    confirmBtn.addEventListener('click', async () => {
        const domain = activeRenewDomain;
        if (!domain || submitting) return;
        if (!currentWallet) { notify('Connect your wallet first.', 'warning'); return; }

        const info = await fetchDomainInfo(domain);
        if (!info) { notify('Could not read this domain from the contract.', 'error'); return; }
        if (!sameWallet(info.owner, currentWallet)) { notify('Only the owner can renew this domain.', 'error'); return; }
        if (!canRenewDomain(info)) {
            notify('The grace period has ended. Anyone can register this name again.', 'error');
            loadMyDomains();
            return;
        }

        const price = await previewPrice();
        if (!price) { notify('Could not read the renewal price from the contract.', 'error'); return; }

        const months = String(parseInt(durationSelect.value, 10));

        try {
            submitting = true;
            confirmBtn.disabled = true;
            confirmBtn.textContent = 'Processing...';

            const retium = await waitForRetium();
            if (!retium) throw new Error('Retium Web Wallet not found.');

            const { txId } = await retium.request({
                method: 'retium_call',
                params: {
                    contractId: CONTRACT_ID,
                    method: 'renew',
                    args: [domain, months],
                    amountAtomic: price
                }
            });

            notify(`Renewal transaction submitted. TX: ${txId}\nWaiting for finalization...`, 'info');
            await waitForFinal(txId);

            addHistory('Renew', domain, `${fromAtomic(price)} RTM (${months} months)`, 'Success');
            notify(`Domain ${domain} renewed for ${months} months.`, 'success');
            modal.style.display = 'none';
            reset();
            loadMyDomains();
            loadMarketplace();
        } catch (err) {
            notify('Renewal failed: ' + (err.message || err), 'error');
        } finally {
            submitting = false;
            confirmBtn.disabled = false;
            confirmBtn.textContent = 'Confirm Renewal';
        }
    });

    modal.addEventListener('click', (event) => {
        if (event.target !== modal) return;
        modal.style.display = 'none';
        reset();
    });

    document.addEventListener('keydown', (event) => {
        if (event.key !== 'Escape') return;
        if (modal.style.display === 'none') return;
        const confirmModal = document.getElementById('confirmModal');
        if (confirmModal && confirmModal.style.display !== 'none') return;
        modal.style.display = 'none';
        reset();
    });

    window.openRenewModal = (domain) => {
        activeRenewDomain = domain;
        label.textContent = `Domain: ${domain}`;
        durationSelect.value = '12';
        modal.style.display = 'flex';
        previewPrice();
    };
}

// --- LOCAL STORAGE ---

function getLocalDB() {
    try {
        const version = Number(localStorage.getItem('rns_db_version') || 0);
        if (version !== DB_VERSION) {
            // v3 reads ownership and listings from the chain, so a cache
            // written for the old contract must not be shown any more.
            localStorage.removeItem('rns_db');
            localStorage.setItem('rns_db_version', String(DB_VERSION));
        }
    } catch (err) {
        console.warn('Could not migrate the local domain cache.', err);
    }

    const db = readJSON('rns_db', {});
    return db && typeof db === 'object' && !Array.isArray(db) ? db : {};
}

function saveDB(db) {
    writeJSON('rns_db', db);
    try { localStorage.setItem('rns_db_version', String(DB_VERSION)); } catch (err) { /* ignore */ }
}

function saveOwnedDomain(domain, owner, months) {
    const db = getLocalDB();
    const previous = db[domain] && typeof db[domain] === 'object' ? db[domain] : {};
    db[domain] = {
        ...previous,
        owner,
        expiresAt: Date.now() + (months * MONTH_MS),
        salePrice: null
    };
    saveDB(db);
}

function listDomainForSaleStorage(domain, price) {
    const atomic = normalizeAtomicPrice(price);
    const db = getLocalDB();
    if (db[domain]) {
        db[domain].salePrice = atomic;
        saveDB(db);
    }
    addHistory('List', domain, `${fromAtomic(atomic || '0')} RTM`, 'Success');
}

function transferDomainStorage(domain, recipient) {
    const db = getLocalDB();
    if (db[domain]) {
        db[domain].owner = recipient;
        db[domain].salePrice = null;
        saveDB(db);
    }
    addHistory('Transfer', domain, `To: ${shortAddress(recipient)}`, 'Success');
}

function buyDomainStorage(domain, buyer) {
    const db = getLocalDB();
    const previousOwner = db[domain] ? db[domain].owner : null;
    if (db[domain]) {
        db[domain].owner = buyer;
        db[domain].salePrice = null;
        saveDB(db);
    }
    addHistory('Buy', domain, previousOwner ? `From: ${shortAddress(previousOwner)}` : 'Marketplace purchase', 'Success');
}

// --- DOMAIN RENDERING ---

function expiryLabel(info) {
    const state = domainState(info);
    if (state === 'active') return `Expires in: ${daysLeft(info.expiresAt)} days`;
    if (state === 'grace') return `Expired; grace period ends in ${graceDaysLeft(info)} days`;
    if (state === 'expired') return 'Expired; available for registration';
    return 'Expiry: unavailable';
}

function stateColor(state) {
    if (state === 'grace') return 'var(--warning)';
    if (state === 'expired') return 'var(--danger)';
    return '';
}

function buildOwnedDomainCard(domain, info) {
    const card = document.createElement('div');
    card.className = 'domain-card';

    const title = document.createElement('h3');
    title.textContent = domain;
    card.appendChild(title);

    const state = domainState(info);
    const salePrice = normalizeAtomicPrice(info.salePrice);
    const status = document.createElement('p');
    if (salePrice !== null) {
        status.append('Status: ');
        const listed = document.createElement('span');
        listed.style.color = 'var(--error-color)';
        listed.textContent = `Listed (${fromAtomic(salePrice)} RTM)`;
        status.appendChild(listed);
    } else if (state === 'grace') {
        status.textContent = 'Status: Grace period';
    } else if (state === 'expired') {
        status.textContent = 'Status: Expired';
    } else {
        status.textContent = 'Status: Active';
    }
    card.appendChild(status);

    const expiry = document.createElement('p');
    expiry.textContent = expiryLabel(info);
    if (stateColor(state)) expiry.style.color = stateColor(state);
    card.appendChild(expiry);

    if (state === 'active' && !canListDomain(info)) {
        const note = document.createElement('p');
        note.textContent = 'Renew before listing: less than 30 days left.';
        note.style.color = 'var(--warning)';
        card.appendChild(note);
    }

    const actions = document.createElement('div');
    actions.className = 'card-actions';

    if (salePrice !== null) {
        actions.appendChild(makeButton(
            'Cancel Listing',
            'btn-sm btn-danger btn-block',
            () => cancelListingAction(domain)
        ));
    } else if (state === 'active') {
        const sell = makeButton('Sell', 'btn-sm btn-sell', () => window.openSellModal(domain));
        if (!canListDomain(info)) {
            sell.disabled = true;
            sell.title = 'Renew this domain before listing it.';
        }
        actions.appendChild(sell);
        actions.appendChild(makeButton('Transfer', 'btn-sm btn-transfer', () => window.openTransferModal(domain)));
    }

    if (canRenewDomain(info)) {
        actions.appendChild(makeButton('Renew', 'btn-sm btn-secondary', () => window.openRenewModal(domain)));
    }

    card.appendChild(actions);
    return card;
}

// Cached entries are only used when the chain cannot be reached.
function cachedDomainInfos(predicate) {
    const db = getLocalDB();
    return Object.entries(db)
        .filter(([, info]) => info && predicate(info))
        .map(([domain, info]) => ({
            domain,
            owner: info.owner,
            target: info.target || '',
            avatar: info.avatar || '',
            salePrice: normalizeAtomicPrice(info.salePrice),
            expiresAt: info.expiresAt,
            isExpired: false,
            isListed: normalizeAtomicPrice(info.salePrice) !== null
        }));
}

function renderLoadMore(kind, barId, next) {
    const bar = document.getElementById(barId);
    if (!bar) return;
    bar.replaceChildren();
    if (!next) return;

    const button = makeButton('Load More', 'btn-secondary', () => {
        if (kind === 'mydomains') loadMyDomains(false);
        else loadMarketplace(false);
    });
    bar.appendChild(button);
}

async function loadMyDomains(reset = true) {
    const container = document.getElementById('mydomainsList');
    if (!container) return;

    const state = pageState.mydomains;
    if (reset) {
        state.cursor = '';
        state.token += 1;
        state.loading = false;
        container.replaceChildren();
        renderLoadMore('mydomains', 'mydomainsMore', '');
    }

    const token = state.token;

    if (!currentWallet) {
        if (reset) emptyMessage(container, 'Connect your wallet to view your domains.');
        return;
    }

    if (!reset && state.loading) return;
    state.loading = true;

    const wallet = currentWallet;
    try {
        const page = await fetchDomainPage('get_domains_by_owner', [wallet, state.cursor, String(PAGE_SIZE)]);
        if (token !== state.token || !sameWallet(wallet, currentWallet)) return;

        if (!page) {
            if (reset) {
                const cached = cachedDomainInfos(info => sameWallet(info.owner, wallet));
                if (cached.length > 0) {
                    cached.sort((a, b) => a.domain.localeCompare(b.domain));
                    cached.forEach(info => container.appendChild(buildOwnedDomainCard(info.domain, info)));
                } else {
                    emptyMessage(container, 'Could not reach the network. Try again later.');
                }
                notify('Could not reach the network. Showing cached domains.', 'warning');
            }
            return;
        }

        const infos = await Promise.all(page.domains.map(fetchDomainInfo));
        if (token !== state.token || !sameWallet(wallet, currentWallet)) return;

        const owned = infos.filter(info => info && sameWallet(info.owner, wallet));
        owned.sort((a, b) => a.domain.localeCompare(b.domain));
        owned.forEach(info => container.appendChild(buildOwnedDomainCard(info.domain, info)));

        state.cursor = page.next;
        const hasCards = container.querySelector('.domain-card') !== null;
        if (!hasCards && !page.next) {
            emptyMessage(container, 'You do not own any domains yet.');
        }
        renderLoadMore('mydomains', 'mydomainsMore', page.next);
    } catch (err) {
        if (token === state.token) {
            console.warn('Could not load owned domains.', err);
            notify('Could not load your domains. Please try again.', 'error');
        }
    } finally {
        if (token === state.token) state.loading = false;
    }
}

function buildMarketplaceCard(info) {
    const domain = info.domain;
    const card = document.createElement('div');
    card.className = 'domain-card';

    const title = document.createElement('h3');
    title.textContent = domain;
    card.appendChild(title);

    const seller = document.createElement('p');
    seller.textContent = `Seller: ${shortAddress(info.owner)}`;
    card.appendChild(seller);

    const price = document.createElement('p');
    price.className = 'price-tag';
    const priceLabel = document.createElement('strong');
    priceLabel.textContent = `${fromAtomic(info.salePrice)} RTM`;
    price.append('Price: ', priceLabel);
    card.appendChild(price);

    const state = domainState(info);
    const expiry = document.createElement('p');
    expiry.textContent = expiryLabel(info);
    if (stateColor(state)) expiry.style.color = stateColor(state);
    card.appendChild(expiry);

    if (currentWallet && sameWallet(info.owner, currentWallet)) {
        card.appendChild(makeButton(
            'Cancel Listing',
            'btn-action',
            () => cancelListingAction(domain)
        ));
    } else if (!canBuyDomain(info)) {
        const note = document.createElement('p');
        note.textContent = state === 'active'
            ? 'Unavailable: less than 30 days left. The seller must renew it first.'
            : 'Unavailable: this listing has expired.';
        note.style.color = 'var(--warning)';
        card.appendChild(note);

        const disabled = makeButton(state === 'active' ? 'Renewal Required' : 'Expired', 'btn-action', () => {});
        disabled.disabled = true;
        card.appendChild(disabled);
    } else {
        card.appendChild(makeButton('Buy Now', 'btn-action', () => buyDomainAction(domain)));
    }

    return card;
}

async function loadMarketplace(reset = true) {
    const container = document.getElementById('marketplaceList');
    if (!container) return;

    const state = pageState.marketplace;
    if (reset) {
        state.cursor = '';
        state.token += 1;
        state.loading = false;
        container.replaceChildren();
        renderLoadMore('marketplace', 'marketplaceMore', '');
    }

    const token = state.token;

    if (!reset && state.loading) return;
    state.loading = true;

    try {
        const page = await fetchDomainPage('get_listed_domains', [state.cursor, String(PAGE_SIZE)]);
        if (token !== state.token) return;

        if (!page) {
            if (reset) {
                const cached = cachedDomainInfos(info => normalizeAtomicPrice(info.salePrice) !== null);
                if (cached.length > 0) {
                    cached.sort((a, b) => a.domain.localeCompare(b.domain));
                    cached.forEach(info => container.appendChild(buildMarketplaceCard(info)));
                } else {
                    emptyMessage(container, 'Could not reach the network. Try again later.');
                }
                notify('Could not reach the network. Showing cached listings.', 'warning');
            }
            return;
        }

        const infos = await Promise.all(page.domains.map(fetchDomainInfo));
        if (token !== state.token) return;

        const listings = infos
            .filter(info => info && info.salePrice !== null)
            .sort((a, b) => a.domain.localeCompare(b.domain));
        listings.forEach(info => container.appendChild(buildMarketplaceCard(info)));

        state.cursor = page.next;
        const hasCards = container.querySelector('.domain-card') !== null;
        if (!hasCards && !page.next) {
            emptyMessage(container, 'No domains are listed for sale right now.');
        }
        renderLoadMore('marketplace', 'marketplaceMore', page.next);
    } catch (err) {
        if (token === state.token) {
            console.warn('Could not load the marketplace.', err);
            notify('Could not load the marketplace. Please try again.', 'error');
        }
    } finally {
        if (token === state.token) state.loading = false;
    }
}

// --- DOMAIN ACTIONS ---

async function buyDomainAction(domain) {
    if (!currentWallet) { notify('Connect your wallet first.', 'warning'); return; }

    const info = await fetchDomainInfo(domain);
    if (!info || info.salePrice === null) {
        notify('Domain listing was not found or is no longer for sale.', 'error');
        loadMarketplace();
        return;
    }
    if (sameWallet(info.owner, currentWallet)) {
        notify('You are the owner of this domain.', 'warning');
        return;
    }
    if (!canBuyDomain(info)) {
        notify('This listing has expired or has less than 30 days left. The seller must renew it first.', 'error');
        loadMarketplace();
        return;
    }

    const price = fromAtomic(info.salePrice);
    const proceed = await confirmAction({
        title: 'Buy Domain',
        message: `Buy ${domain} for ${price} RTM?`,
        confirmLabel: 'Buy Now'
    });
    if (!proceed) return;

    try {
        const retium = await waitForRetium();
        if (!retium) throw new Error('Retium Web Wallet not found.');

        const { txId } = await retium.request({
            method: 'retium_call',
            params: {
                contractId: CONTRACT_ID,
                method: 'buy_domain',
                args: [domain],
                // Pay the exact on-chain listing price in atomic units.
                amountAtomic: info.salePrice
            }
        });

        notify(`Purchase transaction submitted. TX: ${txId}`, 'info');
        await waitForFinal(txId);

        buyDomainStorage(domain, currentWallet);
        notify('Domain purchased successfully!', 'success');
        loadMarketplace();
        loadMyDomains();
    } catch (err) {
        notify('Purchase failed: ' + (err.message || err), 'error');
    }
}

async function cancelListingAction(domain) {
    if (!currentWallet) { notify('Connect your wallet first.', 'warning'); return; }

    const info = await fetchDomainInfo(domain);
    if (!info) { notify('Could not read this domain from the contract.', 'error'); return; }
    if (!sameWallet(info.owner, currentWallet)) { notify('Only the owner can cancel this listing.', 'error'); return; }
    if (info.salePrice === null) { notify('This domain is not listed for sale.', 'warning'); loadMarketplace(); return; }

    const proceed = await confirmAction({
        title: 'Cancel Listing',
        message: `Cancel the sale of ${domain}?`,
        confirmLabel: 'Cancel Listing',
        danger: true
    });
    if (!proceed) return;

    try {
        const retium = await waitForRetium();
        if (!retium) throw new Error('Retium Web Wallet not found.');

        const { txId } = await retium.request({
            method: 'retium_call',
            params: {
                contractId: CONTRACT_ID,
                method: 'list_for_sale',
                args: [domain, '0'],
                amountAtomic: '0'
            }
        });

        notify(`Cancellation transaction submitted. TX: ${txId}\nWaiting for finalization...`, 'info');
        await waitForFinal(txId);

        const db = getLocalDB();
        if (db[domain]) {
            db[domain].salePrice = null;
            saveDB(db);
        }
        addHistory('Cancel Listing', domain, '-', 'Success');

        notify(`Sale of ${domain} cancelled successfully.`, 'success');
        loadMyDomains();
        loadMarketplace();
    } catch (err) {
        notify('Failed to cancel listing: ' + (err.message || err), 'error');
    }
}

// --- TRANSACTION HISTORY ---

function addHistory(action, domain, details, status) {
    txHistory.unshift({
        action,
        domain,
        details,
        timestamp: new Date().toLocaleString('en-US'),
        status,
        wallet: currentWallet || null
    });
    txHistory = txHistory.slice(0, HISTORY_LIMIT);
    writeJSON('rns_history', txHistory);
    renderHistory();
}

function renderHistory() {
    const tbody = document.getElementById('historyTableBody');
    if (!tbody) return;

    tbody.replaceChildren();

    const walletHistory = currentWallet
        ? txHistory.filter(item => sameWallet(item.wallet, currentWallet))
        : [];

    if (!currentWallet || walletHistory.length === 0) {
        const tr = document.createElement('tr');
        const td = document.createElement('td');
        td.colSpan = 5;
        td.className = 'empty-msg';
        td.textContent = currentWallet
            ? 'No transaction history for this wallet yet.'
            : 'Connect your wallet to view its transaction history.';
        tr.appendChild(td);
        tbody.appendChild(tr);
        return;
    }

    walletHistory.forEach(item => {
        const tr = document.createElement('tr');

        const actionTd = document.createElement('td');
        const badge = document.createElement('span');
        badge.className = 'badge-action';
        badge.textContent = item.action || '-';
        actionTd.appendChild(badge);

        const domainTd = document.createElement('td');
        const strong = document.createElement('strong');
        strong.textContent = item.domain || '-';
        domainTd.appendChild(strong);

        const detailsTd = document.createElement('td');
        detailsTd.textContent = item.details || '-';

        const timeTd = document.createElement('td');
        timeTd.textContent = item.timestamp || '-';

        const statusTd = document.createElement('td');
        const status = document.createElement('span');
        status.className = 'status-success';
        status.textContent = item.status || '-';
        statusTd.appendChild(status);

        tr.append(actionTd, domainTd, detailsTd, timeTd, statusTd);
        tbody.appendChild(tr);
    });
}
