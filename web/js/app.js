'use strict';

const $ = s => document.querySelector(s);
const $$ = s => [...document.querySelectorAll(s)];
const node = (tag, className = '', text = '') => { const el = document.createElement(tag); el.className = className; el.textContent = text; return el; };
function icon(name, className = '') { const el = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); const use = document.createElementNS(el.namespaceURI, 'use'); use.setAttribute('href', `#i-${name}`); el.setAttribute('class', className); el.setAttribute('aria-hidden', 'true'); el.append(use); return el; }
function readSaved(key, fallback) { return window.DengPortablePreferences.read(key, fallback); }
function save(key, value) { window.DengPortablePreferences.write(key, value); }
let toastTimer;
let toastLayerObserver;
const toastModalOrder = [];
function placeToast(force = false) {
  const notice = $('#toast'); if (!notice || notice.hidden) return;
  if (window.DengSecurityLock?.isLocked()) { notice.hidePopover?.(); notice.hidden = true; return; }
  const isModal = dialog => { try { return dialog?.isConnected && dialog.matches('dialog:modal'); } catch { return dialog?.open && dialog.getAttribute('aria-modal') !== 'false'; } };
  const focused = document.activeElement?.closest('dialog');
  const modal = [...toastModalOrder].reverse().find(isModal) || (isModal(focused) ? focused : [...document.querySelectorAll('dialog[open]')].reverse().find(isModal));
  const parent = modal || document.body, moved = notice.parentElement !== parent;
  // A high z-index cannot rise above a native dialog backdrop. Keep the live
  // region inside its focus owner; a manual popover escapes scroll clipping.
  if (moved || force) {
    if (typeof notice.hidePopover === 'function' && notice.matches(':popover-open')) notice.hidePopover();
    if (moved) parent.append(notice);
  }
  if (typeof notice.showPopover === 'function') {
    notice.setAttribute('popover', 'manual');
    if (!notice.matches(':popover-open')) notice.showPopover();
  }
}
function toast(message) {
  if (window.DengSecurityLock?.isLocked()) return;
  clearTimeout(toastTimer);
  if (!toastLayerObserver) {
    toastLayerObserver = new MutationObserver(records => {
      for (const {target} of records) if (target.tagName === 'DIALOG') {
        const index = toastModalOrder.indexOf(target); if (index >= 0) toastModalOrder.splice(index, 1);
        if (target.open) toastModalOrder.push(target);
      }
      placeToast(true);
    });
    toastLayerObserver.observe(document.body, {subtree:true, attributes:true, attributeFilter:['open']});
  }
  const notice = $('#toast'); notice.textContent = message; notice.hidden = false; placeToast(true);
  toastTimer = setTimeout(() => {
    if (typeof notice.hidePopover === 'function' && notice.matches(':popover-open')) notice.hidePopover();
    notice.hidden = true; document.body.append(notice);
  }, 4500);
}
const safe = fn => (...args) => { try { return Promise.resolve(fn(...args)).catch(error => toast(error?.message || String(error))); } catch (error) { toast(error?.message || String(error)); } };
const native = () => window.go?.desktop?.Desktop;
function syncDesktopTheme() {
  const desktop = native();
  if (desktop?.SetTheme) desktop.SetTheme(window.DevShellTheme.preference).catch(error => console.warn('无法同步窗口主题', error));
}
function reflectTheme() {
  const dark = window.DevShellTheme.current === 'dark';
  $('#theme-toggle').setAttribute('aria-pressed', String(dark));
  const title = `${dark ? '切换到浅色模式' : '切换到深色模式'}${window.DevShellTheme.preference === 'system' ? '（当前跟随系统，点击后改为手动）' : ''}`;
  $('#theme-toggle').title = title;
  $('#theme-toggle').setAttribute('aria-label', title);
  syncDesktopTheme();
}
$('#theme-toggle').onclick = () => window.DevShellTheme.toggle();
window.addEventListener('cloudshell:theme', reflectTheme);
reflectTheme();
const boot = window.DEVSHELL;
const desktopPage = location.protocol === 'wails:' || location.hostname === 'wails.localhost';
let bridgeReady;
function waitForDesktop() {
  if (!bridgeReady) bridgeReady = (async () => {
    const deadline = Date.now() + 5000;
    while (!native()?.Request) { if (Date.now() >= deadline) throw new Error('桌面接口尚未就绪，请点击服务器管理按钮重试'); await new Promise(resolve => setTimeout(resolve, 40)); }
  })().catch(error => { bridgeReady = null; throw error; });
  return bridgeReady;
}
function endpoint(path) { return boot.base + path; }
async function api(path, options = {}) {
  if (desktopPage) await waitForDesktop();
  if (native()) { try { return JSON.parse(await native().Request(options.method || 'GET', path, options.body || '')); } catch (error) { const message = error?.message || String(error); const marker = message.indexOf('DENGSHELL_ERROR:'); if (marker >= 0) { let data; try { data = JSON.parse(message.slice(marker + 16)); } catch {} if(data?.code==='DENGSHELL_LOCKED')window.DengSecurityLock?.blocked(); if (data) throw Object.assign(new Error(data.error), { code: data.code, hostKey: data.hostKey }); } throw new Error(message); } }
  const response = await fetch(endpoint(path), { ...options, headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': boot.token, ...options.headers } });
  const data = await response.json();
  if(data.code==='DENGSHELL_LOCKED')window.DengSecurityLock?.blocked();
  if (!response.ok) throw Object.assign(new Error(data.error || `请求失败 (${response.status})`), { code: data.code, hostKey: data.hostKey });
  return data;
}
const post = (path, body) => api(path, { method: 'POST', body: JSON.stringify(body) });
const remove = path => api(path, { method: 'DELETE' });
function prettySize(bytes) { if (bytes == null || !Number.isFinite(bytes)) return '—'; if (bytes < 1024) return `${Math.round(bytes)} B`; for (const [power, unit] of [[4, 'TB'], [3, 'GB'], [2, 'MB'], [1, 'KB']]) if (bytes >= 1024 ** power) return `${(bytes / 1024 ** power).toFixed(1)} ${unit}`; }
// Remote entry names and upload paths are literal: a file named "~" must never
// become the home directory. Only explicit navigation opts into home expansion.
function normalizePath(value, base = '/', home = null) { if (home !== null) { if (value === '~') value = home || '/'; else if (value.startsWith('~/')) value = (home || '') + value.slice(1); } const result = []; for (const part of (value.startsWith('/') ? value : `${base}/${value}`).split('/')) { if (!part || part === '.') continue; if (part === '..') result.pop(); else result.push(part); } return '/' + result.join('/'); }
const parentPath = path => normalizePath('..', path);

// Consistent in-app dialogs; serialised so multiple transfer errors cannot overlap.
let dialogQueue = Promise.resolve();
function ask({ title, description = '', input = false, value = '', secret = false, confirm = '确定', signal }) {
  const result = dialogQueue.then(() => new Promise(resolve => {
    if (signal?.aborted) return resolve(null);
    const dialog = $('#action-dialog');
    $('#action-title').textContent = title; $('#action-description').textContent = description;
    $('#action-input-label').hidden = !input; $('#action-input').type = secret ? 'password' : 'text'; $('#action-input').value = value;
    $('#action-confirm').textContent = confirm;
    let done = false;
    const cancel = () => finish(null);
    const finish = answer => { if (done) return; done = true; signal?.removeEventListener('abort', cancel); dialog.close(); resolve(answer); };
    signal?.addEventListener('abort', cancel, { once: true });
    $('#action-form').onsubmit = event => { event.preventDefault(); finish(input ? $('#action-input').value : true); };
    $('#action-cancel').onclick = () => finish(null);
    dialog.oncancel = event => { event.preventDefault(); finish(null); };
    dialog.showModal(); (input ? $('#action-input') : $('#action-confirm')).focus();
  }));
  dialogQueue = result.then(() => undefined); return result;
}

let profiles = [], groups = [], activeID = null, ascending = true, fileSortKey = 'name';
const sessions = new Map(), connecting = new Set(), credentials = new Map(), localTasks = new Map();
// Connections whose credential storage the user explicitly declined in this run
// (edit form unchecked). The connect-time write-back must not undo that choice;
// saving with "remember" checked lifts it again.
const declinedCredentials = new Set();
const temporaryProfiles = new Map();
const connectionRequests = new Map();
const connectionAttempts = new Map();
let nextSessionOrder = 0;
let workspaceInitialized = false;
const current = () => sessions.get(activeID);
const connectionProfile = id => profiles.find(p => p.id === id) || temporaryProfiles.get(id);
const profileFor = session => connectionProfile(session?.profileId);
async function loadProfiles() {
  if (window.DengPortablePreferences.ready) await window.DengPortablePreferences.flush();
  const config = await api('/api/config');
  window.DengPortablePreferences.accept(config);
  window.DengCommandHistory.acceptConfig(config);
  layout = readSaved('cloudshell.layout', { version: 2 });
  serverManager.collapsed = null;
  temporaryProfiles.clear(); for (const p of config.temporaryServers || []) temporaryProfiles.set(p.id, p);
  profiles = config.servers; groups = config.groups; commands = config.commands || []; commandGroups = config.commandGroups || []; managedKeys = config.keys || [];
  acceptServerManagerConfig(config); renderConnections(); renderTabs(); renderCommands(); renderKeyChoices(); renderKeys();
  await acceptAppearanceConfig(config);
  window.DengPinnedNav?.render();
  clampLayouts();
  // Restore once at startup. Sync/config refreshes must not pull an active
  // workspace back to a saved tab, including while a dropped file uploads.
  if (!workspaceInitialized) {
    workspaceInitialized = true;
    const savedPane = readSaved('dengshell.workspace', {}).pane;
    if (['files', 'commands', 'common-apps', 'transfers'].includes(savedPane)) showPane(savedPane, false);
    setWorkspaceVisible(layout.filesHidden === false, false);
  }
  window.DevShellHelp?.acceptConfig(config);
}

function sessionDisplayName(state) {
  const name = profileFor(state)?.name || '服务器';
  if (!state) return name;
  const peers = [...sessions.values()].filter(s => s.profileId === state.profileId).sort((a, b) => (a.tabOrder ?? 0) - (b.tabOrder ?? 0));
  return peers.length > 1 ? `${name} · 会话 ${peers.indexOf(state) + 1}` : name;
}
function releaseConnectionAttempt(state) {
  const key = state.connectionRequestKey;
  if (connectionAttempts.get(key) !== state) return;
  connectionAttempts.delete(key); connectionRequests.delete(key);
  if (![...connectionAttempts.values()].some(s => s.profileId === state.profileId)) connecting.delete(state.profileId);
}
async function connect(profileID, force = false, options = {}) {
  if (connectionProfile(profileID)?.protocol === "rdp") return window.DengRDP.connect(profileID, {...options, previousID: force ? options.sessionId : null});
  if (connectionProfile(profileID)?.protocol === "serial") return window.DengSerial.connect(profileID, {...options, previousID: force ? options.sessionId : null});
  const matches = [...sessions.values()].filter(s => s.profileId === profileID);
  const previous = force ? (options.sessionId ? sessions.get(options.sessionId) : matches.find(s => s.id === activeID) || matches[0]) : null;
  // Reconnect keeps the same profile in its own tab. A disconnected tab may
  // also be replaced in place by a different profile (local-prompt "ssh ...").
  if (force && (!previous || previous.closed || (previous.profileId !== profileID && (previous.connected || previous.pendingConnection)))) return null;
  // Coalesce repeat clicks for the same action, not independent reconnects.
  const requestKey = force ? `reconnect:${previous.id}` : `new:${profileID}`;
  if (connectionRequests.has(requestKey)) return connectionRequests.get(requestKey);
  const request = connectProfile(profileID, { ...options, previous, requestKey });
  connectionRequests.set(requestKey, request);
  try { return await request; } finally { if (connectionRequests.get(requestKey) === request) connectionRequests.delete(requestKey); }
}
// A credential typed this run for a saved connection that stores none: write it
// back so the next start, a pinned chip or a double click reconnects silently.
// replaceStored additionally permits overwriting a stored credential the server
// has just rejected. Temporary quick-connect profiles have no stored row yet
// (their own save dialogs own the credential), and agent/managed-key auth has no
// profile secret to write: a library key's passphrase belongs to the key
// manager, not to the connection, and is never rewritten from here.
async function persistRunCredential(profile, secret, { replaceStored = false } = {}) {
  if (!secret || !profile || (profile.hasSecret && !replaceStored) || temporaryProfiles.has(profile.id) || declinedCredentials.has(profile.id)) return false;
  if (profile.auth === 'agent' || (profile.auth === 'key' && profile.keyId)) return false;
  const saved = await post('/api/profiles', { ...profile, secret, clearSecret: false });
  const index = profiles.findIndex(entry => entry.id === saved.id);
  if (index >= 0) profiles[index] = saved; else profiles.push(saved);
  return saved.hasSecret === true;
}
async function connectProfile(profileID, { background = false, refreshHistory = true, previous = null, requestKey } = {}) {
  const profile = connectionProfile(profileID); if (!profile) return;
  if (profile.auth === 'key' && !profile.keyId && !profile.keyPath) {
    toast(`🔑  「${profile.name}」未找到私钥，请重新配置密钥。`);
    if (!background) showConnectionForm(profile);
    return null;
  }
  // An ordinary open always starts a new SSH session. Reconnect replaces only
  // its explicitly captured source, never another tab with the same profile.
  const existing = previous;
  if (existing?.detaching || existing?.handoffProvisional || existing?.ownershipUncertain) { toast('⏳  此 SSH 正在交接窗口，请稍后再试'); return null; }
  const startPage = !background && !existing ? window.DengStartTabs?.take() : null;
  const tabOrder = existing?.tabOrder ?? startPage?.tabOrder ?? nextSessionOrder++;
  const state = makeSessionState({ id: `pending:${profileID}:${crypto.randomUUID()}`, profileId: profileID, home: '/' });
  Object.assign(state, { connectionRequestKey: requestKey, tabOrder, connected: false, localOnly: true, pendingConnection: true, announceConnectionReady: true, connectionAbort: new AbortController() });
  const alive = () => !state.closed && sessions.get(state.id) === state;
  connecting.add(profileID); connectionAttempts.set(requestKey, state);
  // Removing an old session is independent of every other selected profile.
  const previousClose = existing ? preserveReconnectTerminal(existing, state) : Promise.resolve();
  sessions.set(state.id, state);
  try {
    createTerminal(state);
    if (!background || (!activeID && !window.DengRDP?.active())) activate(state.id); else renderTabs();
    if (!background) setDrawer(false);
    renderConnections();
    await previousClose;
    if (!alive()) return null;
    await restoreReconnectTerminal(state);
    if (!alive()) return null;
    showConnectionProgress(state, '🔗  连接主机...');
    let secret = credentials.get(profileID) || '', credentialRetried = false;
    // Managed key credentials are resolved by Go first. Another window may
    // have saved its passphrase since this window last loaded the key list.
    if (!secret && !profile.hasSecret && (profile.auth === 'password' || (profile.auth === 'key' && !profile.keyId))) {
      showConnectionProgress(state, '🔑  等待输入凭据...');
      // These are exactly the auth modes persistRunCredential can write back, so
      // say up front that a successful connection stores the credential (a
      // not-yet-saved quick connection has no row to write into).
      const promptNote = (profile.auth === 'key' ? '如果私钥没有加密，可直接确定。' : `${profile.user}@${profile.host}:${profile.port}`) + (temporaryProfiles.has(profileID) || declinedCredentials.has(profileID) ? '' : '\n连接成功后凭据会加密保存到该连接，之后自动连接。');
      secret = await ask({ title: profile.auth === 'key' ? `私钥口令 · ${profile.name}` : `连接 ${profile.name}`, description: promptNote, input: true, secret: true, confirm: '连接', signal: state.connectionAbort.signal });
      if (!alive()) return null;
      if (secret === null) { showConnectionProgress(state, '🚫  已取消连接', true); return null; }
      if (profile.auth === 'password' && !secret) throw new Error('请输入 SSH 密码');
    }
    showConnectionProgress(state, '🔗  连接主机...');
    if (profile.verbose) state.term.writeln(`\r\n[debug] 已开启 -${'v'.repeat(profile.verbose)} 诊断，正在建立 SSH 连接。`);
    const requestConnection = async () => {
      let hostKeyApproval;
      for (let confirmations = 0; ; confirmations++) {
        try { return await api('/api/sessions', { method: 'POST', body: JSON.stringify({ profileId: profileID, secret, hostKeyApproval }), signal: state.connectionAbort.signal }); }
        catch (error) {
          if (!alive()) return null;
          if (!['ssh_host_key_unknown', 'ssh_host_key_changed'].includes(error.code) || !error.hostKey || confirmations >= 2) throw error;
          const key = error.hostKey, changed = error.code === 'ssh_host_key_changed';
          showConnectionProgress(state, '🛡️  等待核实服务器指纹...');
          const accepted = await ask({ title: changed ? `服务器指纹已变化 · ${profile.name}` : `首次连接 · ${profile.name}`, description: `${key.host}\n算法：${key.algorithm}\n${changed ? `原指纹：${key.previousFingerprint}\n` : ''}新指纹：${key.fingerprint}\n\n${changed ? '服务器重装、密钥更换或连接被冒充都可能导致此变化。' : ''}请通过服务器控制台或可信管理员核对指纹；确认一致后才继续发送登录凭据。`, confirm: '已核实，信任并连接', signal: state.connectionAbort.signal });
          if (!alive()) return null;
          if (!accepted) { showConnectionProgress(state, '🚫  已取消连接', true); return null; }
          hostKeyApproval = { host: key.host, fingerprint: key.fingerprint, previousFingerprint: key.previousFingerprint };
          showConnectionProgress(state, '🔗  连接主机...');
        }
      }
    };
    let info;
    try { info = await requestConnection(); }
    catch (error) {
      if (!alive()) return null;
      credentials.delete(profileID);
      const needsKeyPassphrase = profile.auth === 'key' && ['ssh_key_passphrase_required', 'ssh_key_passphrase_invalid'].includes(error.code);
      if (!needsKeyPassphrase && !(profile.auth === 'password' && error.code === 'ssh_authentication_failed')) throw error;
      showConnectionProgress(state, '🔑  等待重新输入凭据...');
      // The stored credential was just rejected by the server, so a credential
      // that connects now replaces it instead of staying run-only. Managed-key
      // passphrases stay owned by the key manager and are never overwritten here.
      const replacesStored = profile.auth !== 'key' || !profile.keyId;
      credentialRetried = replacesStored;
      secret = await ask({ title: `${needsKeyPassphrase ? (error.code === 'ssh_key_passphrase_required' ? '私钥口令' : '重新输入私钥口令') : '重新输入 SSH 密码'} · ${profile.name}`, description: error.message + `\n本次输入会替代缓存凭据${replacesStored ? '；连接成功后同时替换该连接已保存的旧凭据。' : '，不会修改密钥管理器中保存的口令。'}`, input: true, secret: true, confirm: '连接', signal: state.connectionAbort.signal });
      if (!alive()) return null;
      if (secret === null) { showConnectionProgress(state, '🚫  已取消连接', true); return null; }
      if (profile.auth === 'password' && !secret) throw new Error('请输入 SSH 密码');
      showConnectionProgress(state, '🔗  连接主机...');
      info = await requestConnection();
    }
    if (!info) return null;
    // The user may close/retry this tab while a native request is still running.
    // Dispose only that late result; never resurrect its placeholder or touch a newer attempt.
    if (!alive()) { await remove(`/api/sessions/${info.id}`).catch(() => {}); return null; }
    const pendingID = state.id, selected = activeID === pendingID;
    sessions.delete(pendingID);
    Object.assign(state, info, { cwd: info.home, connected: true, localOnly: false, pendingConnection: false });
    if (profile.verbose) startVerboseDiagnostics(state, profile.verbose);
    sessions.set(state.id, state); state.host.dataset.session = state.id;
    state.openTerminalSocket();
    // Echo ssh -L outcomes the way OpenSSH reports them: a bound forward is
    // announced, a failed bind warns without ever killing the session.
    if (info.forwards?.length) for (const forward of info.forwards) state.term.writeln(forward.error ? `\r\n⚠️  端口转发未开启：${forward.listen} → ${forward.remote}（${forward.error}）` : `\r\n🔀 本地端口转发已开启：${forward.listen} → ${forward.remote}（经 ${profile.host}）`);
    // Each tab becomes usable independently, preserving order and the user's current focus.
    if (selected) activate(state.id); else renderTabs();
    if (secret) credentials.set(profileID, secret);
    // A credential typed for a saved connection used to live only in this run's
    // memory, so the same connection asked again after every restart. Persist it
    // once the connection actually succeeded; failure only means asking again.
    if (secret && (!profile.hasSecret || credentialRetried)) {
      try {
        if (await persistRunCredential(profile, secret, { replaceStored: credentialRetried })) toast(credentialRetried ? `🔑  「${profile.name}」已用本次验证通过的凭据替换保存的旧凭据` : `🔑  「${profile.name}」凭据已加密保存到本机，下次直接连接`);
      } catch { /* 保存失败不改变已建立的会话 */ }
    }
    window.DengCommandHistory?.refresh(profileID).catch(error => toast('⚠️  命令历史刷新失败：' + error.message));
    if (current() === state && state.ready) { pollStats(); pollLatency(); }
    // A slow or failed SFTP listing must not block any other SSH connection.
    if (state.sftpPending) void refreshFileStatus(state, true);
    else if (state.sftpAvailable !== false) navigate(info.home, state).catch(error => { if (!state.closed) toast(`⚠️  ${profile.name} 已连接，目录读取失败：${error.message}`); });
    if (refreshHistory) safe(refreshServerManagerHistory)();
    return state;
  } catch (error) {
    if (alive()) {
      showConnectionProgress(state, '❌  连接主机失败：' + (error.message || String(error)), true); if (profile.verbose) startVerboseDiagnostics(state, profile.verbose, `handshake-${profileID}`); toast(`❌  ${profile.name}：${error.message || error}`);
      if (!background && ['ssh_key_missing', 'ssh_key_unavailable', 'ssh_private_key_invalid', 'ssh_proxy_missing'].includes(error.code)) showConnectionForm(profile);
    }
    return null;
  } finally {
    releaseConnectionAttempt(state);
    if (alive() && state.localOnly) { state.pendingConnection = false; renderTabs(); if (current() === state) renderSessionInfo(); }
    renderConnections(); updateStatus();
  }
}
function writeTerminalAndWait(state, text = '') {
  if (state.closed) return Promise.resolve(false);
  return new Promise((resolve, reject) => {
    state.terminalWriteWaiters ||= new Set();
    const done = () => { state.terminalWriteWaiters.delete(done); resolve(!state.closed); };
    state.terminalWriteWaiters.add(done);
    try { state.term.write(text, done); }
    catch (error) { state.terminalWriteWaiters.delete(done); reject(error); }
  });
}
async function preserveReconnectTerminal(previous, next) {
  // Drain xterm's asynchronous write queue, including the final output frame
  // and shell-confirmed history events, before disposing the old renderer.
  await writeTerminalAndWait(previous);
  if (!previous.closed) {
    next.reconnectScreen = {
      cols: previous.term.cols, rows: previous.term.rows,
      // A new SSH shell must not inherit an old TUI's mouse/paste/alternate
      // screen modes. Only rendered normal-buffer text and colors are copied.
      text: previous.serialize.serialize({ excludeAltBuffer: true, excludeModes: true }),
    };
    await closeSession(previous.id);
  }
}
async function restoreReconnectTerminal(state) {
  const screen = state.reconnectScreen;
  if (!screen || state.closed) return;
  state.term.resize(screen.cols, screen.rows);
  await writeTerminalAndWait(state, screen.text + '\x1b[0m\r\n\x1b[38;5;245m🔄  ── 重新连接 · 以上为上一会话记录 ──\x1b[0m\r\n');
  state.reconnectScreen = null;
  if (current() === state && !state.closed) fitActive();
}
function showConnectionProgress(state, message, failed = false) {
  if (state.closed) return;
  const changed = state.connectionMessage !== message || state.connectionFailed !== failed;
  state.connectionMessage = message; state.connectionFailed = failed;
  // These messages are local text, not remote terminal instructions. Keep
  // errors readable without allowing control bytes to erase earlier output.
  if (changed) state.term.writeln(String(message).replace(/\r\n?/g, '\n').replace(/[\x00-\x09\x0b-\x1f\x7f-\x9f]/g, '').replace(/\n/g, '\r\n'));
  if (current() === state) renderSessionInfo();
}
function startVerboseDiagnostics(state, level, traceID = state.id) {
  let cursor = 0, stopped = false, finalPolls = 0;
  const clean = value => String(value || '').replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');
  state.term.writeln(`\r\n[debug] 显示 DevShell 诊断（级别 ${level}）；与系统 OpenSSH 的 debug 输出不同。`);
  const poll = async () => {
    if (stopped || state.closed) return;
    try {
      const result = await api(`/api/sessions/${encodeURIComponent(traceID)}/verbose?after=${cursor}`);
      if (state.closed) return;
      for (const event of result.events || []) {
        cursor = Math.max(cursor, event.sequence || 0);
        state.term.writeln(`\r\n[debug +${Number(event.ageSeconds || 0).toFixed(2)}s] ${clean(event.kind)}${event.detail ? ': ' + clean(event.detail) : ''}`);
      }
    } catch { /* A transient local request failure must not affect SSH. */ }
    if (!state.connected && ++finalPolls >= 3) { stopped = true; return; }
    setTimeout(poll, state.connected ? 1000 : 200);
  };
  void poll();
}
function makeSessionState(info) { return { ...info, connected: true, ready: false, cwd: info.home, entries: [], folders: new Map(), expanded: new Set(), history: getCommandHistory(info.profileId), historyIndex: getCommandHistory(info.profileId).length, navGeneration: 0, stats: null, chart: [], latency: null, follow: false, ws: null }; }
// Desktop IPC keeps WebKit independent of system proxy and loopback policies.
class DesktopSocket {
  constructor(id, handoffNonce = '', opener = '') {
    this.readyState = WebSocket.CONNECTING; let finished = false; let writes = Promise.resolve();
    const finish = () => { if (finished) return; finished = true; this.readyState = WebSocket.CLOSED; off(); this.onclose?.(); };
    const off = window.runtime.EventsOn('cloudshell:terminal:' + id, message => {
      if ((message.handoff || '') !== handoffNonce) return;
      if (message.kind === 0) { finish(); return; }
      const bytes = Uint8Array.from(atob(message.data), c => c.charCodeAt(0));
      this.onmessage?.({ data: message.kind === 2 ? bytes.buffer : new TextDecoder().decode(bytes) });
    });
    this.send = data => { writes = writes.then(() => (native().SendTerminalWithHandoff ? native().SendTerminalWithHandoff(id, handoffNonce, data) : native().SendTerminal(id, data))).catch(() => finish()); };
    this.close = () => { (native().CloseTerminalWithHandoff ? native().CloseTerminalWithHandoff(id, handoffNonce) : native().CloseTerminal(id)).finally(finish); };
    queueMicrotask(() => {
      this.readyState = WebSocket.OPEN;
      // Serial tabs dial the serial terminal endpoint through the same IPC pipe.
      const request = opener === 'serial' ? native().OpenSerialTerminal(id) : native().OpenTerminalWithHandoff ? native().OpenTerminalWithHandoff(id, handoffNonce) : native().OpenTerminal(id);
      request.catch(error => { toast(String(error)); this.onerror?.(); finish(); });
    });
  }
}

function createTerminal(state) {
  const host = node('div', 'terminal-session'); host.dataset.session = state.id; host.hidden = true; $('#terminal-output').append(host); state.host = host;
  // Preserve intentional low-contrast ANSI art (including black on black).
  // Improve readability through the backdrop, never by recoloring its cells.
  const terminal = new Terminal({ cols: state.restoration?.cols || 100, rows: state.restoration?.rows || 30, fontSize: terminalFont, fontFamily: terminalFontFamily, fontWeight: boldForFont() ? '700' : '400', fontWeightBold: '700', lineHeight: 1.25, cursorBlink: !matchMedia('(prefers-reduced-motion: reduce)').matches, scrollback: 10000, allowTransparency: true, minimumContrastRatio: 1, theme: terminalTheme() });
  const fit = new FitAddon.FitAddon(); terminal.loadAddon(fit); const serialize = new SerializeAddon.SerializeAddon(); terminal.loadAddon(serialize); state.serialize = serialize; terminal.open(host); state.term = terminal; state.fit = fit;
  if (state.localOnly) terminal.options.disableStdin = true;
  installTerminalFontMetrics(state);
  installTerminalMouseCoordinates(state);
  installTerminalScrollback(state);
  bindTerminalContext(state);
  bindShellIntegration(state);
  if (state.restoration?.clientState?.shellIntegration) state.shellIntegration = { ...state.restoration.clientState.shellIntegration };
  terminal.onData(data => sendInput(state, data));
  terminal.onResize(({ cols, rows }) => { if (!state.detaching && !state.restoring) sendMessage(state, { type: 'resize', cols, rows }); });
  terminal.attachCustomKeyEventHandler(event => {
    if (event.type !== 'keydown') return true;
    if (event.ctrlKey && !event.altKey && event.code === 'KeyC') {
      event.preventDefault();
      if (event.shiftKey) copyTerminalSelection(state).catch(() => toast('无法访问剪贴板'));
      else { terminal.clearSelection(); terminal.scrollToBottom(); sendInput(state, '\x03'); }
      return false;
    }
    if (event.ctrlKey && !event.altKey && event.shiftKey && event.code === 'KeyV') { event.preventDefault(); pasteClipboard(state).catch(() => toast('无法访问剪贴板')); return false; }
    return true;
  });
  terminal.parser.registerOscHandler(7, data => { if (!state.follow) return false; try { const url = new URL(data); if (url.protocol === 'file:') { const path = decodeURIComponent(url.pathname); if (path !== state.cwd) navigate(path, state).catch(error => toast(error?.message || String(error))); } } catch {} return true; });
  const openSocket = () => {
  if (state.closed) return;
  const socketURL = new URL(endpoint(`${state.serial ? '/api/serial/sessions/' : '/api/sessions/'}${state.id}/terminal`)); socketURL.protocol = socketURL.protocol === 'https:' ? 'wss:' : 'ws:'; socketURL.searchParams.set('token', boot.token); socketURL.searchParams.set('cols', String(terminal.cols)); socketURL.searchParams.set('rows', String(terminal.rows)); if (state.handoffNonce) socketURL.searchParams.set('handoff', state.handoffNonce);
  const ws = native() ? new DesktopSocket(state.id, state.handoffNonce || '', state.serial ? 'serial' : '') : new WebSocket(socketURL); ws.binaryType = 'arraybuffer'; state.ws = ws;
  ws.onmessage = event => {
    if (state.closed || !state.connected || sessions.get(state.id) !== state) return;
    if (event.data instanceof ArrayBuffer) {
      const firstOutput = !state.terminalOutputStarted; state.terminalOutputStarted = true;
      const bytes = new Uint8Array(event.data);
      terminal.write(bytes, () => {
        // OSC command/cwd reports can share this frame with the first ZModem
        // header. Parse them before consulting the bridge, or sz has no intent
        // until lrzsz retries its header several seconds later.
        window.DengZmodemBridge?.observe(state, bytes);
        sendMessage(state, { type: 'ack' });
        if (firstOutput && current() === state && state.connected) { pollStats(); pollLatency(); }
      });
      return;
    }
    const message = JSON.parse(event.data);
    if (window.DengSessionWindows.handleMessage(state, message)) return;
    if (message.type === 'ready') {
      // The relay sends ready only after the remote shell starts, before its
      // first output. Window handoffs must not add text to a running shell.
      if (state.announceConnectionReady) { state.announceConnectionReady = false; showConnectionProgress(state, state.serial ? '✅  串口已打开！' : '✅  连接主机成功！'); if (!state.serial && state.sftpAvailable === false && !state.sftpPending) showConnectionProgress(state, state.sftpDisabled ? 'ℹ️  Dropbear 连接默认关闭 SFTP，SSH 终端可正常使用。' : 'ℹ️  SFTP 文件服务不可用，SSH 终端可正常使用。'); }
      if (state.sftpPending) void refreshFileStatus(state);
      acceptShellIntegration(state, message.integration); state.ready = true; applyPendingFollow(state); state.handoffProvisional = false; state.ownershipUncertain = false; terminal.options.disableStdin = !!state.detaching || !state.connected; state.restoring = false; state.restoration = null;
      if (!state.terminalOutputStarted && !state.terminalBackgroundTimer) state.terminalBackgroundTimer = setTimeout(() => { state.terminalBackgroundTimer = null; if (!state.closed && state.connected && sessions.get(state.id) === state) { state.terminalBackgroundReady = true; if (current() === state) { pollStats(); pollLatency(); } } }, 1000);
      if (activeID === state.id) renderSessionInfo(); if (activeID === state.id) { fitActive(); terminal.focus(); }
    }
    else if (message.message) terminal.writeln(`\r\n\x1b[38;5;245m${message.type === 'error' ? '❌' : message.type === 'exit' ? '🔌' : 'ℹ️'}  ${message.message.replaceAll('\x1b', '')}\x1b[0m`);
  };
  ws.onclose = () => { if (state.ownershipUncertain && !state.detaching) dropSessionView(state.id); else if (!state.detaching) markSessionDisconnected(state); };
  ws.onerror = () => toast('❌  终端连接失败，请重新连接');
  };
  state.openTerminalSocket = openSocket;
  if (state.localOnly) return;
  if (state.restoration) terminal.write(state.restoration.terminal, () => {
    try { DengTerminalSnapshot.restore(terminal, state.restoration.clientState.terminalRuntime); openSocket(); }
    catch (error) { state.windowRestoreError = error; if (state.initialWindowRestore) { toast(error.message); dropSessionView(state.id, state); post('/api/windows/handoff/'+state.handoffNonce+'/cancel',{}).then(result=>{if(!result.attached&&!native())window.close()}).catch(()=>{}); } }
  }); else openSocket();
}
function sendMessage(state, value) { if (state.ws?.readyState === WebSocket.OPEN) state.ws.send(JSON.stringify(value)); }
function sendInput(state, data) {
  if (!state.connected || !state.ready || state.detaching || state.restoring || state.ownershipUncertain) return;
  // Any input can leave an unfinished line, start a command, or enter a TUI.
  // Only the next authenticated shell prompt makes automatic command injection
  // safe again; do not guess prompt state from local echo or key sequences.
  if (data.length && state.shellIntegration) state.shellIntegration.atPrompt = false;
  for (let start = 0; start < data.length;) { let end = Math.min(start + 16000, data.length); if (end < data.length && /[\uD800-\uDBFF]/.test(data[end - 1])) end--; sendMessage(state, { type: 'input', data: data.slice(start, end) }); start = end; }
}
function fitActive() { const state = current(); if (state && !state.detaching && !state.restoring && !state.host.hidden && state.host.clientWidth > 0 && state.host.clientHeight > 0) { state.fit.fit(); if (state.ready) sendMessage(state, { type: 'resize', cols: state.term.cols, rows: state.term.rows }); } }
new ResizeObserver(() => requestAnimationFrame(fitActive)).observe($('#terminal-output'));
function markSessionDisconnected(state) {
  if (state.closed || !state.connected) return;
  state.connected = false; state.ready = false; state.navGeneration++; state.navAbort?.abort();
  state.term.options.disableStdin = true;
  if (state.serial) state.term.writeln('\r\n\x1b[38;5;245m🔌  串口已关闭，终端内容已保留。点击重新连接可重新打开串口。\x1b[0m');
  else state.term.writeln('\r\n\x1b[38;5;245m🔌  连接已断开，终端内容已保留。点击重新连接可建立新会话。\x1b[0m');
  state.term.writeln('\x1b[38;5;245m💡  本机提示符已就绪：在下方输入框输入 ssh 用户@主机 连接新地址，reconnect 重连本会话，exit 关闭标签；↑ / ↓ 回看本机输入过的命令。\x1b[0m');
  // The remote shell is gone and terminal stdin is disabled: a blinking cursor
  // would suggest the terminal still takes input, so hide it. Reconnect builds
  // a fresh terminal where the cursor is visible again.
  state.term.write('\x1b[?25l');
  // Serial sessions have no SSH disconnect diagnostic endpoint; the reason is
  // already visible (device unplugged, port closed, or the tab was closed).
  if (!state.serial) showDisconnectDiagnostic(state);
  renderTabs();
  if (activeID === state.id) { renderSessionInfo(); renderFiles(); $('#command-input').focus(); }
}
async function showDisconnectDiagnostic(state) {
  if (state.disconnectDiagnosticRequested) return;
  state.disconnectDiagnosticRequested = true;
  let result;
  try {
    for (let attempt = 0; attempt < 4; attempt++) {
      let timer;
      try {
        result = await Promise.race([
          api(`/api/sessions/${state.id}/disconnect-diagnostic`),
          new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('诊断接口未响应')), 2000); }),
        ]);
      } finally { clearTimeout(timer); }
      if (result.code || state.closed) break;
      await new Promise(resolve => setTimeout(resolve, 150));
    }
  } catch {}
  if (state.closed) return;
  if (!/^DS-\d{3}$/.test(result?.code || '')) result = { code: 'DS-290', traceId: state.id.slice(0, 12), message: '无法取得后端断开原因，可能是本地窗口通信异常；请提供本地诊断日志。' };
  state.disconnectDiagnostic = result;
  const clean = value => String(value || '').replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');
  const label = `断开诊断码 ${result.code}${result.traceId ? ' / ' + clean(result.traceId) : ''}`;
  state.term.writeln(`\r\n\x1b[38;5;214m${['DS-100', 'DS-101'].includes(result.code) ? 'ℹ️' : '⚠️'}  [${label}]\x1b[0m ${clean(result.message)}`);
  if (result.logPath) state.term.writeln(`📄  诊断日志：${clean(result.logPath)}${result.logWriteFailed ? '（文件写入失败，请保留本页诊断码）' : ''}`);
  if (current() === state) {
    $('#terminal-state').textContent = `已断开 · ${result.code}`;
    $('#terminal-state').title = label + ' · ' + clean(result.message);
    $('#terminal-state').hidden = false; $('#terminal-current-badge').hidden = true;
  }
}
async function disconnectSession(state = current()) {
  if (!state?.connected || state.disconnecting || state.detaching || state.handoffProvisional || state.ownershipUncertain) return;
  state.disconnecting = true;
  if (activeID === state.id) $('#disconnect').disabled = true;
  try {
    await remove(state.serial ? `/api/serial/sessions/${state.id}` : `/api/sessions/${state.id}`);
    markSessionDisconnected(state); state.ws?.close();
  } finally {
    state.disconnecting = false;
    if (activeID === state.id) { $('#disconnect').disabled = !state.connected; $('#reconnect').disabled = false; }
  }
}
// Xshell-style fallback: after the remote shell exits, the tab stays and the
// command bar becomes a local prompt. Commands run locally; "ssh ..." connects
// a new address in this same tab instead of forcing a close-and-reopen.
async function sessionLocalCommand(state, raw) {
  const command = raw.trim();
  const echo = () => state.term.writeln(`\r\n\x1b[38;5;114m[本机] $ ${command}\x1b[0m`);
  if (/^(exit|quit|close)$/i.test(command)) { echo(); await closeSession(state.id); return; }
  if (/^reconnect$/i.test(command)) { echo(); await connect(state.profileId, true, { sessionId: state.id }); return; }
  if (/^help$/i.test(command)) {
    echo();
    for (const hint of ['ssh 用户@主机 —— 在本标签连接新地址（支持 ssh -p 端口 用户@主机、ssh -vvv 连接诊断、IPv6）', 'reconnect —— 用原地址重新连接本标签', 'ping 主机 [-t] —— 本机探测，Ctrl+C 停止', 'traceroute 主机 —— 本机路由追踪，逐跳实时输出', 'nslookup / dig 主机 —— 本机 DNS 解析与查询', 'curl [-I] 网址 —— 本机 http/https 请求', '运行中的本机命令随时按 Ctrl+C 停止', 'exit —— 关闭本标签']) state.term.writeln(`\x1b[38;5;245m  ${hint}\x1b[0m`);
    return;
  }
  if (window.DengLocalPrompt.commands.test(command)) { await window.DengLocalPrompt.run(command, window.DengLocalPrompt.termSink(state.term)); return; }
  echo();
  const profile = await post('/api/quick-connect', { command, auth: 'password', keyId: '' });
  temporaryProfiles.set(profile.id, profile);
  await connect(profile.id, true, { sessionId: state.id });
}
function dropSessionView(id, expected = null) {
  const state = sessions.get(id); if (!state || expected && state !== expected) return;
  state.closed = true; state.connected = false; state.ready = false;
  for (const done of state.terminalWriteWaiters || []) done();
  if (state.localOnly) {
    state.connectionAbort?.abort();
    releaseConnectionAttempt(state);
  }
  window.DengProcessView?.drop(id);
  state.navAbort?.abort(); state.ws?.close(); state.term?.dispose(); state.host?.remove(); sessions.delete(id);
  if (activeID === id) activeID = sessions.keys().next().value || window.DengStartTabs?.first() || null;
  window.dispatchEvent(new CustomEvent('dengshell:session-context-change'));
  if (activeID) activate(activeID, window.DengProcessView?.active() === activeID ? 'processes' : 'terminal'); else { renderTabs(); renderSessionInfo(); renderFiles(); updateTransferNotice(); }
}
async function closeSession(id) {
  if (window.DengStartTabs?.has(id)) return window.DengStartTabs.close(id);
  if (window.DengRDP?.has(id)) return window.DengRDP.close(id);
  if (window.DengSerial?.has(id)) return window.DengSerial.close(id);
  const state = sessions.get(id); if (!state || state.detaching || state.handoffProvisional) return;
  dropSessionView(id);
  if (state.ownershipUncertain || state.localOnly) return;
  await remove(`/api/sessions/${id}`).catch(() => {});
}
function renderTabs() {
  if (window.DengSessionWindows.deferTabsRender()) return;
  $('#session-tabs').setAttribute('role', 'tablist');
  $('#session-tabs').replaceChildren(...[...sessions.values()].sort((a,b) => (a.tabOrder ?? 0) - (b.tabOrder ?? 0)).flatMap(state => {
    const selected = state.id === activeID && !window.DengProcessView?.active();
    const profile = profileFor(state); const tab = node('div', `session-tab${selected ? ' active' : ''}`);
    tab.dataset.sessionId = state.id;
    tab.dataset.connecting = String(!!state.pendingConnection);
    tab.dataset.failed = String(!!state.connectionFailed);
    const label = node('span', 'session-tab-text');
    label.append(node('span', 'session-tab-label', profile ? sessionDisplayName(state) : '已删除配置'));
    const button = node('button'); button.type = 'button'; button.append(node('span', `status-dot ${state.connected ? 'green' : 'blue'}`), label); button.onclick = () => activate(state.id); button.setAttribute('role', 'tab'); button.setAttribute('aria-selected', String(selected)); button.setAttribute('aria-current', selected ? 'page' : 'false'); button.title = profile ? (profile.protocol === 'serial' ? `${profile.name} · ${window.DengSerial.label(profile)}` : `${profile.name} · ${profile.user}@${profile.host}:${profile.port}`) : '已删除配置';
    if (selected && state.connected) tab.classList.add('current-session');
    button.setAttribute('aria-busy', String(!!state.pendingConnection));
    if (state.localOnly) button.title += ` · ${state.connectionMessage}`;
    const close = node('button', 'tab-close'); close.append(icon('close')); close.title = state.pendingConnection ? '取消连接' : '关闭连接'; close.setAttribute('aria-label', `${state.pendingConnection ? '取消' : '关闭'} ${profile?.name || '会话'}`); close.onclick = safe(() => closeSession(state.id)); close.disabled = !!(state.detaching || state.handoffProvisional); tab.dataset.detaching = String(!!state.detaching); tab.append(button, close);
    // 已断开的标签双击即用原地址在本标签重连，与 #reconnect 按钮语义一致；
    // 连接中、交接中或已删除配置的标签不响应，关闭按钮上的双击不触发。
    tab.ondblclick = event => { if (event.target.closest('.tab-close')) return; if (profile && !state.connected && !state.pendingConnection && !state.disconnecting && !state.detaching && !state.handoffProvisional && !state.ownershipUncertain) safe(() => connect(state.profileId, true, { sessionId: state.id }))(); };
    window.DengSessionWindows.bindTab(tab, state); return [tab, ...(window.DengProcessView?.tabs(state) || [])];
  })); window.DengStartTabs?.appendTabs($('#session-tabs')); window.DengRDP?.appendTabs($('#session-tabs')); window.DengTabOrder.apply($('#session-tabs')); updateStatus(); window.DengTextEditors?.reflect(); window.DengCommandComposer?.reflect(); window.DengPinnedNav?.render();
}
function activate(id, view = 'terminal') {
  if (window.DengStartTabs?.has(id)) return window.DengStartTabs.activate(id);
  if (!sessions.has(id)) return;
  window.DengRDP?.deactivate();
  activeID = id; $('#file-filter').value = '';
  window.dispatchEvent(new CustomEvent('dengshell:session-context-change'));
  window.DengProcessView?.activate(id, view);
  for (const state of sessions.values()) state.host.hidden = state.id !== id;
  renderTabs(); renderSessionInfo(); renderFiles(); updateTransferNotice();
  requestAnimationFrame(() => {
    if (activeID !== id) return;
    $('#session-tabs .session-tab.active')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    if (!window.DengProcessView?.active()) { fitActive(); current()?.term.focus(); }
  });
}
function updateStatus() { $('#connection-button').setAttribute('aria-label', '管理服务器'); window.DengSessionWindows.reflect(); }
function renderSessionInfo() {
  const state = current(), profile = profileFor(state);
  $('#welcome-state').hidden = !!state; window.DengLocalPrompt?.reflect();
  $('#terminal-meta-host').textContent = profile ? (profile.protocol === 'serial' ? window.DengSerial.label(profile) : `${profile.user}@${sessionDisplayName(state)}`) : 'SSH 终端'; $('#terminal-state').textContent = state?.handoffProvisional ? '正在接收标签…' : state?.pendingConnection ? '正在连接…' : state?.connectionFailed ? '连接未完成' : state?.connected ? '已连接' : state?.disconnectDiagnostic ? `已断开 · ${state.disconnectDiagnostic.code}` : '待连接';
  $('#terminal-state').title = state?.disconnectDiagnostic?.message || '';
  // The "当前连接" badge lives in the session header, merged with the connected
  // status: the green dot plus badge replaces the plain "已连接" text.
  const currentConnected = !!state?.connected && !state?.handoffProvisional;
  $('#terminal-state').hidden = currentConnected; $('#terminal-current-badge').hidden = !currentConnected;
  const localPrompt = !!state && !state.connected && !state.pendingConnection && !state.disconnecting && !state.detaching && !state.handoffProvisional && !state.ownershipUncertain;
  $('#command-history').disabled = false; $('#command-input').disabled = !!state && !(state.connected || localPrompt); $('#command-input').placeholder = state ? (state.connected ? '输入命令并回车发送 · ↑↓ 历史' : localPrompt ? '本机提示符：ssh · reconnect · ping/traceroute/nslookup/dig/curl · exit · ↑↓ 本机历史' : '输入命令并回车发送 · ↑↓ 历史') : '本机：输入 ssh 用户@主机 或 ping/traceroute/nslookup/dig/curl · ↑↓ 本机历史'; window.DengQuickConnect?.reflect(); $('#command-input').value = ''; resizeCommandInput(); $('#reconnect').disabled = !state || !!(state.pendingConnection || state.disconnecting || state.detaching || state.handoffProvisional || state.ownershipUncertain); $('#disconnect').disabled = !state?.connected || !!(state.disconnecting || state.detaching || state.handoffProvisional || state.ownershipUncertain);
  renderMonitor(state?.connected ? state.stats : null); renderLatency(); renderCommands(); window.DengCommonApps?.render(); window.DengProcessView?.reflect(); window.DengPinnedNav?.reflect(); updateStatus();
}
$('#command-form').onsubmit = event => { event.preventDefault(); const state = current(); const input = $('#command-input'); if (!state && input.value.trim()) { const command = input.value; input.value = ''; window.DengCommandHistory.recordLocal(command); if (window.DengLocalPrompt.commands.test(command.trim())) safe(() => window.DengLocalPrompt.run(command))(); else safe(() => window.DengQuickConnect.run(command))(); return; } if (state && !state.connected && !state.pendingConnection && !state.detaching && !state.handoffProvisional && !state.ownershipUncertain && input.value.trim()) { const command = input.value; input.value = ''; window.DengCommandHistory.recordLocal(command); safe(() => sessionLocalCommand(state, command))(); return; } if (!state?.ready || !input.value) return; if (!pasteTerminalText(state, input.value, { execute: true })) return; input.value = ''; resizeCommandInput(); };
$('#command-input').onkeydown = event => { if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); $('#command-form').requestSubmit(); return; } if (!['ArrowUp', 'ArrowDown'].includes(event.key)) return; const state = current(); event.preventDefault(); const direction = event.key === 'ArrowUp' ? -1 : 1; if (state?.connected) { state.historyIndex = Math.max(0, Math.min(state.history.length, state.historyIndex + direction)); event.target.value = state.history[state.historyIndex] || ''; } else { event.target.value = window.DengCommandHistory.recallLocal(event.target.value, direction); } resizeCommandInput(); };
$('#reconnect').onclick = safe(() => { const state = current(); return state && connect(state.profileId, true, { sessionId: state.id }); });
$('#disconnect').onclick = safe(() => disconnectSession());
$('#clear-terminal').onclick = () => { if (window.DengLocalPrompt?.handleClear()) return; current()?.term.clear(); current()?.term.focus(); };
let terminalFont = normalizeTerminalFontSize(appearance.terminalFontSize);
function changeFont(amount) { terminalFont = normalizeTerminalFontSize(terminalFont + amount); document.documentElement.style.setProperty('--terminal-font-size', `${terminalFont}px`); for (const state of sessions.values()) state.term.options.fontSize = terminalFont; persistAppearance({ terminalFontSize: terminalFont }).catch(error => toast(error.message)); renderAppearanceControls(); fitActive(); }
$('#font-up').onclick = () => changeFont(.5); $('#font-down').onclick = () => changeFont(-.5);
// 跟随终端目录：开关在设置的外观页（默认开启）。开启后连接一旦 Bash / Zsh /
// Fish 提示符集成就绪就自动跟随；关闭立即停止全部会话的跟随，重新开启会对已
// 就绪的会话重新生效。设置持久化，重启后保持。
function followTerminalEnabled() { return appearance.followTerminal !== false; }
function applyPendingFollow(state) {
  if (!followTerminalEnabled() || !state?.connected || state.follow || !state.shellIntegration?.ready) return;
  state.follow = true; renderSessionInfo();
  if (filesUsable(state) && state.terminalDirectory && state.terminalDirectory !== state.cwd) navigate(state.terminalDirectory, state).catch(error => toast(error?.message || String(error)));
}
function reflectTerminalFollow() {
  for (const state of sessions.values()) {
    if (followTerminalEnabled()) applyPendingFollow(state);
    else state.follow = false;
  }
}

function filesUsable(state = current()) { return !!state?.connected && !state.sftpPending && state.sftpAvailable !== false; }

// The status request waits independently of the terminal socket. A delayed or
// absent file subsystem leaves the shell usable and updates this panel later.
function refreshFileStatus(state, useHome = false) {
  if (state.fileStatusRequest || !state.connected || state.closed) return state.fileStatusRequest;
  const alive = () => !state.closed && state.connected && sessions.get(state.id) === state;
  state.fileStatusRequest = (async () => {
    try {
      const info = await api(`/api/sessions/${state.id}/file-status`);
      if (!alive()) return;
      Object.assign(state, info);
      if (info.sftpPending) { setTimeout(() => { if (alive()) void refreshFileStatus(state, useHome); }, 250); return; }
      if (activeID === state.id) { renderSessionInfo(); renderFiles(); }
      if (info.sftpAvailable) await navigate(state.follow && state.terminalDirectory ? state.terminalDirectory : (useHome ? info.home : state.cwd), state);
    } catch (error) {
      if (!alive()) return;
      // A status transport error is not proof that the remote SFTP service is
      // unavailable. Keep the pending display and retry while this tab lives.
      if (state.sftpPending) setTimeout(() => { if (alive()) void refreshFileStatus(state, useHome); }, 1000);
      else toast('⚠️  目录读取失败：' + error.message);
    } finally { state.fileStatusRequest = null; }
  })();
  return state.fileStatusRequest;
}

// SFTP browser. Only successful requests replace the current path and entries.
async function navigate(path, state = current()) {
  if (!filesUsable(state) || state.closed || sessions.get(state.id) !== state) return;
  path = normalizePath(path, state.cwd, state.home || '/');
  const generation = ++state.navGeneration; state.navAbort?.abort(); state.navAbort = new AbortController();
  const currentRequest = () => state.navGeneration === generation && state.connected && !state.closed && sessions.get(state.id) === state;
  if (activeID === state.id) $('#file-status-count').textContent = '读取目录…';
  try {
    const data = await api(`/api/sessions/${state.id}/files?path=${encodeURIComponent(path)}`, { signal: state.navAbort.signal });
    if (!currentRequest()) return;
    if (state.cwd !== data.path) state.fileSelection = null;
    state.cwd = data.path; state.entries = data.entries; state.folders.set(data.path, data.entries.filter(entry => entry.kind === 'folder')); DengFileBrowser.invalidate(state, data.path);
    if (data.historyError) toast(data.historyError);
    if (activeID === state.id) { $('#file-filter').value = ''; renderFiles(); }
  } catch (error) { if (error.name === 'AbortError' || !currentRequest()) return; if (activeID === state.id) { $('#path-input').value = state.cwd; $('#file-status-count').textContent = '目录读取失败'; } throw error; }
}
function renderFiles() {
  window.DengPathHistory?.reflect();
  window.DengDirectoryFavorites?.reflect();
  const state = current(); const filter = $('#file-filter').value.toLowerCase();
  const entries = (state?.entries || []).filter(e => e.name.toLowerCase().includes(filter)).slice().sort((a, b) => compareFileEntries(a, b, fileSortKey, ascending));
  $('#path-input').value = state?.cwd || ''; $('#path-input').disabled = !filesUsable(state); $('#drop-path').textContent = state?.cwd || '—';
  $('#file-status-count').textContent = state ? `${entries.length} 个项目${filter ? ' · 已筛选' : ''}` : '尚未连接'; $('#file-empty').hidden = !!entries.length; $('#file-empty').textContent = state ? '此目录暂无匹配文件' : '连接后浏览远程文件';
  for (const id of ['parent-directory', 'refresh-files', 'mkdir', 'choose-files']) $(`#${id}`).disabled = !filesUsable(state);
  if (state?.sftpPending) { $('#file-status-count').textContent = '正在连接文件服务…'; $('#file-empty').textContent = '正在加载远程文件，终端可正常使用。'; }
  else if (state?.sftpAvailable === false) { $('#file-status-count').textContent = state.sftpDisabled ? 'SFTP 已关闭' : 'SFTP 不可用'; $('#file-empty').textContent = state.sftpDisabled ? 'Dropbear 连接默认关闭 SFTP 文件功能，SSH 终端可正常使用。' : '当前服务器的 SFTP 文件服务不可用，SSH 终端可正常使用。'; }
  $('#parent-directory').disabled ||= state?.cwd === '/';
  const selection = fileSelection(state), visibleNames = new Set(entries.map(entry => entry.name));
  for (const name of selection.names) if (!visibleNames.has(name)) selection.names.delete(name);
  if (!visibleNames.has(selection.anchor)) selection.anchor = null;
  $('#file-list').replaceChildren(...entries.map(entry => {
    const row = node('tr'); row.tabIndex = 0; row.dataset.fileName = entry.name; row.setAttribute('aria-label', entry.name);
    const first = node('td'); const name = node('span', 'file-name'); name.append(icon(entry.kind === 'folder' ? 'folder' : 'file', `${entry.kind}-icon`), node('span', 'file-name-text', entry.name + (entry.link ? ' ↗' : ''))); first.append(name);
    row.append(first, node('td', '', entry.kind === 'folder' ? '—' : prettySize(entry.bytes)), node('td', '', entry.time), node('td', '', entry.mode), node('td', '', entry.owner));
    for (const cell of row.cells) cell.title = cell.textContent;
    row.onclick = event => selectFileRow(state, entry.name, event);
    const open = safe(async () => { const target = normalizePath(entry.name, state.cwd); if (entry.kind === 'folder') await navigate(target, state); else if (entry.link) { try { await navigate(target, state); } catch { await DengFileTools.openText(state, target); } } else await DengFileTools.openText(state, target); });
    row.ondblclick = event => { if (!event.ctrlKey && !event.metaKey && !event.shiftKey) open(); };
    row.onkeydown = event => { if (event.key === 'Enter' && selectedEntries(state).length <= 1) { event.preventDefault(); open(); } }; return row;
  })); renderTree(); reflectFileSelection();
}
// Selection belongs to one SSH session and one directory. Never carry hidden
// files into a filtered selection or reuse names after navigation.
function fileSelection(state = current()) {
  if (!state) return { names: new Set(), anchor: null };
  if (state.fileSelection?.path !== state.cwd) state.fileSelection = { path: state.cwd, names: new Set(), anchor: null };
  return state.fileSelection;
}
function selectedEntries(state = current()) {
  const names = fileSelection(state).names;
  return (state?.entries || []).filter(entry => names.has(entry.name));
}
function selectedFileTargets(state = current()) {
  return selectedEntries(state).map(entry => ({ path: normalizePath(entry.name, state.cwd), isDirectory: entry.kind === 'folder' }));
}
function selectFileRow(state, name, event = {}) {
  if (state !== current() || !filesUsable(state)) return;
  const selection = fileSelection(state), additive = event.ctrlKey || event.metaKey;
  const names = $$('#file-list tr').map(row => row.dataset.fileName);
  if (event.shiftKey && selection.anchor !== null && names.includes(selection.anchor)) {
    const start = names.indexOf(selection.anchor), end = names.indexOf(name);
    if (!additive) selection.names.clear();
    for (const value of names.slice(Math.min(start, end), Math.max(start, end) + 1)) selection.names.add(value);
  } else {
    if (!additive) selection.names.clear();
    if (additive && selection.names.has(name)) selection.names.delete(name); else selection.names.add(name);
    selection.anchor = name;
  }
  reflectFileSelection();
}
function reflectFileSelection() {
  const state = current(), selection = fileSelection(state);
  for (const row of $$('#file-list tr')) {
    const selected = selection.names.has(row.dataset.fileName);
    row.classList.toggle('selected', selected); row.setAttribute('aria-selected', String(selected));
  }
  if (filesUsable(state)) $('#file-status-count').textContent = `${$('#file-list').rows.length} 个项目${$('#file-filter').value ? ' · 已筛选' : ''}${selection.names.size ? ` · 已选 ${selection.names.size} 项` : ''}`;

}
function deleteSelectedFiles() { const state = current(); return DengFileTools.removeFiles(state, selectedFileTargets(state)); }
$('#file-list').addEventListener('keydown', event => {
  if (!filesUsable() || event.isComposing || event.altKey) return;
  const selection = fileSelection(), rows = $$('#file-list tr'), index = rows.indexOf(event.target.closest('tr'));
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
    event.preventDefault(); event.stopPropagation(); selection.names = new Set(rows.map(row => row.dataset.fileName)); reflectFileSelection();
  } else if (event.key === 'Escape') {
    event.preventDefault(); selection.names.clear(); selection.anchor = null; reflectFileSelection();
  } else if (event.key === 'Delete') {
    event.preventDefault(); safe(deleteSelectedFiles)();
  } else if (event.key === ' ') {
    event.preventDefault(); if (index >= 0) selectFileRow(current(), rows[index].dataset.fileName, event);
  } else if (['ArrowDown', 'ArrowUp'].includes(event.key)) {
    event.preventDefault(); const row = rows[Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))];
    if (row) { row.focus(); if ((!event.ctrlKey && !event.metaKey) || event.shiftKey) selectFileRow(current(), row.dataset.fileName, event); }
  }
});
function renderTree() { window.DengFileBrowser.render(current()); }
$('#path-form').onsubmit = safe(async event => { event.preventDefault(); await navigate($('#path-input').value); });
$('#parent-directory').onclick = safe(() => navigate(parentPath(current().cwd)));
$('#refresh-files').onclick = safe(() => navigate(current().cwd));
$('#file-filter').oninput = renderFiles;
function compareFileEntries(a, b, key = 'name', asc = true) {
  if ((a.kind === 'folder') !== (b.kind === 'folder')) return a.kind === 'folder' ? -1 : 1;
  const direction = asc ? 1 : -1;
  if (key !== 'name') {
    const field = key === 'size' ? 'bytes' : 'modifiedAt';
    const left = a[field], right = b[field];
    const leftValid = typeof left === 'number' && Number.isFinite(left), rightValid = typeof right === 'number' && Number.isFinite(right);
    if (leftValid !== rightValid) return leftValid ? -1 : 1;
    if (leftValid && left !== right) return (left < right ? -1 : 1) * direction;
  }
  return a.name.localeCompare(b.name) * direction;
}
for (const [id, key] of [['sort-files', 'name'], ['sort-size', 'size'], ['sort-time', 'time']]) {
  document.getElementById(id).onclick = () => {
    ascending = fileSortKey === key ? !ascending : true;
    fileSortKey = key;
    updateFileSortHeaders();
    renderFiles();
  };
}
function updateFileSortHeaders() {
  for (const [id, key, label] of [['sort-files', 'name', '文件名'], ['sort-size', 'size', '大小'], ['sort-time', 'time', '修改时间']]) {
    const button = document.getElementById(id), active = fileSortKey === key;
    button.closest('th').setAttribute('aria-sort', active ? (ascending ? 'ascending' : 'descending') : 'none');
    button.querySelector('span').textContent = active ? (ascending ? '↑' : '↓') : '↕';
    button.title = `按${label}${active && ascending ? '降序' : '升序'}排列`;
  }
}
updateFileSortHeaders();
$('#mkdir').onclick = event => window.DengFileTools.showCreateMenu(event.currentTarget);
async function renameRemoteFile(state, path) {
  if (!state?.connected) return;
  const previousName = path.split('/').filter(Boolean).at(-1) || '';
  const name = await ask({ title: '重命名', input: true, value: previousName });
  if (!name || name === previousName) return;
  if (!state.connected || !sessions.has(state.id)) return toast('此会话已断开，请重新连接');
  await post(`/api/sessions/${state.id}/file-action`, { action: 'rename', path, name });
  DengFileBrowser.invalidate(state, path.slice(0, path.lastIndexOf('/')) || '/');
  await navigate(state.cwd, state);
}

// The backend samples every connected SSH independently. The visible tab only
// reads its cached history; one slow view request cannot block another tab.
let statsTimer = 0, networkTimer = 0, networkPolledSession = '';
const statsRequests = new Set();
const networkRequests = new Set();
function monitorVisible() { return !document.hidden && !window.DevShellWindowHidden && !window.DengSecurityLock?.isLocked(); }
function monitorPollDelay(value, interval) { return Number.isFinite(value) ? Math.max(0, Math.min(interval, value)) + 5 : interval; }
async function pollNetwork() {
  if(window.DengSecurityLock?.isLocked())return;
  const state = current(); if (!state?.connected || state.serial || (!state.terminalOutputStarted && !state.terminalBackgroundReady) || networkRequests.has(state) || !monitorVisible()) return;
  clearTimeout(networkTimer); networkTimer = 0; networkRequests.add(state); networkPolledSession = state.id;
  let delay = 1000;
  try {
    const stats = await api(`/api/sessions/${state.id}/network`);
    if (sessions.get(state.id) !== state || !state.connected) return;
    const sampledAt = Date.parse(stats.sampledAt), previous = Date.parse(state.networkStats?.sampledAt);
    rememberNetworkSample(state, stats);
    if (!state.networkStats || Number.isFinite(sampledAt) && (!Number.isFinite(previous) || sampledAt >= previous)) {
      const { history, ...latest } = stats; state.networkStats = latest; state.networkError = stats.sampleError || '';
    }
    // The next remote deadline is advanced before its SSH result arrives.
    // Retry only the local cache while that sample is in flight, instead of
    // missing its completion and showing the previous rate for another second.
    delay = stats.sampleInProgress ? 100 : Math.max(100, monitorPollDelay(stats.nextSampleInMilliseconds, 1000));
  } catch (error) { if (sessions.get(state.id) === state && state.connected) state.networkError = error.message || String(error); }
  finally {
    networkRequests.delete(state);
    if (current() === state && state.connected && monitorVisible()) {
      renderNetwork(state.stats);
      networkTimer = setTimeout(() => { networkTimer = 0; pollNetwork(); }, delay);
    }
  }
}
async function pollStats() {
  const state = current(); if (!state?.connected || state.serial || (!state.terminalOutputStarted && !state.terminalBackgroundReady) || statsRequests.has(state) || !monitorVisible()) return;
  if ((!networkTimer && !networkRequests.has(state)) || networkPolledSession !== state.id) pollNetwork();
  clearTimeout(statsTimer); statsTimer = 0; statsRequests.add(state); const includeProcesses = true;
  let delay = 5000;
  try { const stats = await api(`/api/sessions/${state.id}/stats?processes=${includeProcesses ? 1 : 0}`); if (sessions.get(state.id) !== state || !state.connected) return; rememberProcessSample(state, stats); state.stats = stats; delay = monitorPollDelay(stats.nextSampleInMilliseconds, 5000); if (activeID === state.id) { renderMonitor(stats); $('.system-section').classList.remove('stale'); $('#monitor-state').title = ''; } }
  catch (error) { if (activeID === state.id) { $('#monitor-state').textContent = '读取失败'; $('#monitor-state').hidden = false; $('#monitor-state').title = error.message; $('.system-section').classList.add('stale'); } }
  finally { statsRequests.delete(state); if (current() === state && state.connected && monitorVisible()) { clearTimeout(statsTimer); statsTimer = setTimeout(() => { statsTimer = 0; pollStats(); }, delay); } }
}
function meter(selector, percent, detail, valid = true) { const el = $(selector); el.querySelector('i').style.width = `${valid ? Math.max(0, Math.min(100, percent)) : 0}%`; el.querySelector('b').textContent = valid ? `${percent.toFixed(1)}%` : '—'; el.querySelector('em').textContent = detail; }
// Discrete GPUs only: the backend never reports integrated graphics, so an
// empty list hides the whole block. Multi-GPU hosts get GPU0, GPU1, ... rows.
function renderGPUs(stats) {
  const gpus = stats?.gpus || [], box = $('#gpu-metrics');
  box.hidden = !gpus.length;
  if (!gpus.length) { box.replaceChildren(); return; }
  box.replaceChildren(...gpus.map((gpu, index) => {
    const util = Math.max(0, Math.min(100, Number(gpu.util) || 0));
    const row = node('div', 'metric gpu-row');
    row.title = `${gpu.name || 'GPU'} · 实际占用 ${util.toFixed(1)}% · 显存 ${prettySize(gpu.memUsed || 0)} / ${prettySize(gpu.memTotal || 0)}`;
    const meter = node('div', 'meter');
    const fill = node('i', 'gpu-fill'); fill.style.width = `${util}%`;
    meter.append(fill, node('b', '', `${util.toFixed(1)}%`), node('em', '', gpu.memTotal ? `显存 ${prettySize(gpu.memUsed)} / ${prettySize(gpu.memTotal)}` : '显存未知'));
    row.append(node('span', '', `GPU${index}`), meter);
    return row;
  }));
}
function renderDisks(stats) {
  // Focus the sidebar on large mounted filesystems. Use the same binary
  // units as prettySize; aliases and virtual volumes are filtered upstream.
  const disks = (stats?.disks || []).filter(disk => disk.total > 10 * 1024 ** 3);
  const box = $('#disk-filesystems');
  box.hidden = !disks.length;
  box.replaceChildren(...disks.map(disk => {
    const percent = disk.used / disk.total * 100;
    const row = node('div', 'metric disk-filesystem');
    row.title = `挂载点：${disk.path}\n已用 ${prettySize(disk.used)} / 总计 ${prettySize(disk.total)}，可用 ${prettySize(disk.available)}`;
    const label = node('span', '', disk.path); label.title = disk.path;
    const bar = node('div', 'meter');
    const fill = node('i'); fill.style.width = `${Math.max(0, Math.min(100, percent))}%`;
    bar.append(fill, node('b', '', `${percent.toFixed(1)}%`), node('em', '', `${prettySize(disk.used)} / ${prettySize(disk.total)}`));
    row.append(label, bar);
    return row;
  }));
}
function renderMonitor(stats) {
  if (window.DengSystemIdentity) window.DengSystemIdentity.render(stats?.os);
  else { $('#system-os').textContent = stats?.os || '—'; $('#system-os').title = stats?.os || ''; }
  $('#cpu-model').textContent = stats?.cpuModel || '—'; $('#cpu-model').title = stats?.cpuModel || ''; $('#cpu-cores').textContent = stats ? `${stats.cores} vCPU` : '—';
  $('#uptime').textContent = stats ? `${Math.floor(stats.uptime / 86400)} 天 ${Math.floor(stats.uptime % 86400 / 3600)} 时` : '—'; $('#system-load').textContent = stats?.load || '—';
  meter('#cpu-meter', stats?.cpu || 0, stats ? `${stats.cores} 核心` : '—', !!stats?.sampleReady);
  meter('#memory-meter', stats?.memoryTotal ? stats.memoryUsed / stats.memoryTotal * 100 : 0, stats ? `${prettySize(stats.memoryUsed)} / ${prettySize(stats.memoryTotal)}` : '—', !!stats);
  meter('#swap-meter', stats?.swapTotal ? stats.swapUsed / stats.swapTotal * 100 : 0, stats ? `${prettySize(stats.swapUsed)} / ${prettySize(stats.swapTotal)}` : '—', !!stats);
  renderGPUs(stats);
  renderProcesses(stats);
  renderDisks(stats);
  $('#disk-read').textContent = stats?.sampleReady ? prettySize(stats.diskRead) + '/s' : '—'; $('#disk-write').textContent = stats?.sampleReady ? prettySize(stats.diskWrite) + '/s' : '—';
  renderNetwork(stats);
  renderServerAddresses(stats);
  $('#system-load').title = '系统负载约每 5 秒刷新';
  if (current()?.connected && networkPolledSession !== current().id) queueMicrotask(pollNetwork);
}
function drawCharts(samples) {
  renderTrafficChart($('#network-chart'),samples);
}
const latencyRequests = new Set();
async function pollLatency() {
  const state = current(); if (!state?.connected || state.serial || (!state.terminalOutputStarted && !state.terminalBackgroundReady) || latencyRequests.has(state) || !monitorVisible()) return;
  latencyRequests.add(state);
  try {
    const sample = await api(`/api/sessions/${state.id}/latency`); if (sessions.get(state.id) !== state || !state.connected) return;
    state.latency = sample;
  } catch { if (sessions.get(state.id) === state && state.connected) state.latency = null; }
  finally { latencyRequests.delete(state); if (current() === state && state.connected && monitorVisible()) renderLatency(); }
}
function renderLatency() { renderLatencyDetails(); }
setInterval(pollLatency, 1000);
setInterval(() => {
  if (!monitorVisible()) { clearTimeout(networkTimer); clearTimeout(statsTimer); networkTimer = statsTimer = 0; return; }
  if (!current()?.connected || current().serial) return;
  if (!networkRequests.has(current()) && !networkTimer) pollNetwork();
  if (!statsRequests.has(current()) && !statsTimer) pollStats();
  renderNetwork(current().stats);
}, 1000);
document.addEventListener('visibilitychange', () => { if (!document.hidden) { pollStats(); pollNetwork(); } });

// Connection groups and credentials are persisted by Go, never in browser storage.
let drawerTrigger;
function setDrawer(open) { if (open) drawerTrigger = document.activeElement; $('#connections-drawer').hidden = !open; $('#drawer-backdrop').hidden = !open; if (open) { renderConnections(); $('#connection-search').focus(); safe(refreshServerManagerHistory)(); } else { closeServerGroupMenu(); drawerTrigger?.focus(); } }
for (const id of ['connection-button', 'welcome-connect']) $(`#${id}`).onclick = () => setDrawer(true);
$('#add-session').onclick = () => window.DengStartTabs.create();
$('#close-connections').onclick = () => setDrawer(false); $('#drawer-backdrop').onclick = () => setDrawer(false);
$('#connection-search').oninput = renderConnections;
function renderConnections() { renderServerManager(); }
function showConnectionForm(profile = null, groupID = '') {
  if (profile?.temporary) { window.DengQuickConnect.open(`ssh -p ${profile.port} ${profile.user}@${profile.host}`); return; }
  const form = $('#connection-form'); form.reset(); form.elements.id.value = profile?.id || '';
  renderKeyChoices(); renderProxyChoices(); form.elements.proxyId.value = profile?.proxyId || '';
  for (const key of ['name', 'notes', 'host', 'user', 'port', 'auth', 'keyPath', 'keyId', 'protocol', 'domain', 'serialPort', 'baudRate', 'dataBits', 'parity', 'stopBits', 'flowControl']) if (profile?.[key] != null) form.elements[key].value = profile[key];
  form.elements.rdpClipboard.checked = !!profile?.rdpClipboard;
  window.DengProfileNotes.init(form.elements.notes, profile?.notes || '');
  form.elements.proxyType.value = profile?.proxy?.type || 'direct';
  for (const [field, key] of [['proxyHost', 'host'], ['proxyPort', 'port'], ['proxyUser', 'user']]) form.elements[field].value = profile?.proxy?.[key] || '';
  form.elements.proxyPassword.placeholder = profile?.proxy?.hasPassword ? '已保存，留空保留' : '代理密码（可选）';
  $('#proxy-settings').open = !!profile?.needsProxy; updateProxyFields();
  chooseConnectionGroup(profile, groupID);
  // Credentials are remembered by default in every save path (this form,
  // quick-connect save, link-bar pin); unchecking is the explicit opt-out and
  // also clears a previously stored secret. A profile connected this run with
  // a typed password keeps it cached in memory either way.
  form.elements.remember.checked = true; $('#saved-secret-note').hidden = !profile?.hasSecret; $('#reset-host-key').hidden = !profile;
  $('#connection-form-title').textContent = profile?.needsProxy ? '编辑连接 · 请补充代理或确认直连' : profile ? '编辑连接' : '新建连接'; $('#save-connection').textContent = profile ? '保存更改' : '保存并连接'; updateAuthFields(); if (profile?.protocol === 'serial') void window.DengSerial.loadPorts(); $('#connection-dialog').showModal(); $('#connection-form .connection-form-body').scrollTop = 0;
}
function updateAuthFields() {
  const form = $('#connection-form'), auth = form.elements.auth.value, key = managedKeys.find(key => key.id === form.elements.keyId.value);
  const rdp = form.elements.protocol.value === 'rdp', serial = form.elements.protocol.value === 'serial';
  // Serial profiles carry no address, login, or proxy fields; normalizeSerialProfile
  // clears them in Go, so hide the inputs and drop their required flags.
  $('#connection-address-row').hidden = serial; $('#connection-login-section').hidden = serial; $('#proxy-settings').hidden = serial;
  $('#serial-settings').hidden = !serial; $('#serial-params').hidden = !serial;
  form.elements.host.required = !serial; form.elements.port.required = !serial; form.elements.user.required = !serial; form.elements.serialPort.required = serial;
  $('#connection-auth-field').hidden = rdp; $('#rdp-domain-field').hidden = !rdp; $('#rdp-clipboard-field').hidden = !rdp; $('#rdp-connection-note').hidden = !rdp;
  $('#reset-host-key').hidden = rdp || serial || !form.elements.id.value;
  const needsSecret = auth === 'password' || (auth === 'key' && (!key || (key.encrypted && !key.hasPassphrase)));
  $('#key-library-field').hidden = auth !== 'key'; $('#key-path-field').hidden = auth !== 'key' || !!form.elements.keyId.value;
  $('#secret-field').hidden = !needsSecret; $('#remember-secret-field').hidden = !needsSecret;
  $('#saved-secret-note').hidden = !needsSecret || !profiles.find(profile => profile.id === form.elements.id.value)?.hasSecret;
  $('#managed-key-secret-note').hidden = auth !== 'key' || !key?.hasPassphrase;
  $('#secret-label').textContent = auth === 'key' ? '私钥口令' : '密码'; $('#choose-key').hidden = !native();
}
$('#connection-form').elements.auth.onchange = updateAuthFields;
$('#connection-protocol').onchange = () => {
  const form = $('#connection-form'), protocol = form.elements.protocol.value, rdp = protocol === 'rdp', serial = protocol === 'serial';
  if (rdp) form.elements.auth.value = 'password';
  if (!serial && ['22','3389',''].includes(form.elements.port.value)) form.elements.port.value = rdp ? '3389' : '22';
  if (!form.elements.id.value && ['root','Administrator',''].includes(form.elements.user.value)) form.elements.user.value = rdp ? 'Administrator' : 'root';
  if (serial) window.DengSerial.loadPorts();
  updateAuthFields();
};
$('#new-connection').onclick = () => showConnectionForm(); $('#cancel-connection').onclick = () => $('#connection-dialog').close();
$('#choose-key').onclick = safe(async () => { const path = await native().ChooseKey(); if (path) $('#connection-form').elements.keyPath.value = path; });
$('#connection-form').onsubmit = safe(async event => {
  event.preventDefault(); const form = event.target, values = Object.fromEntries(new FormData(form)), isNew = !values.id;
  if (!window.DengProfileNotes.validate(form.elements.notes)) return;
  const key = managedKeys.find(key => key.id === values.keyId), needsSecret = values.auth === 'password' || (values.auth === 'key' && (!key || (key.encrypted && !key.hasPassphrase)));
  // Empty secret field plus "remember" keeps the credential used this run
  // (in-memory cache) instead of silently dropping it.
  const secret = needsSecret ? (values.secret || credentials.get(values.id) || '') : '', remember = needsSecret && form.elements.remember.checked;
  const previous = profiles.find(profile => profile.id === values.id);
  const proxy = { type: values.proxyId ? 'direct' : values.proxyType, host: values.proxyHost, port: Number(values.proxyPort), user: values.proxyUser, password: values.proxyPassword, clearPassword: form.elements.clearProxyPassword.checked };
  for (const name of ['proxyType', 'proxyHost', 'proxyPort', 'proxyUser', 'proxyPassword', 'clearProxyPassword']) delete values[name];
  // Serial profiles mirror the device name into Host so search and history keep
  // working; the Go normalizer clears the network-only fields.
  const serial = values.protocol === 'serial';
  if (serial) values.host = String(values.serialPort || '').trim();
  else for (const name of ['serialPort', 'baudRate', 'dataBits', 'parity', 'stopBits', 'flowControl']) delete values[name];
  const saved = await post('/api/profiles', { ...values, rdpClipboard: form.elements.rdpClipboard.checked, port: serial ? 0 : Number(values.port), baudRate: Number(values.baudRate) || 0, dataBits: Number(values.dataBits) || 0, stopBits: Number(values.stopBits) || 0, secret: remember ? secret : '', clearSecret: !remember, proxy });
  credentials.delete(saved.id);
  if (secret) credentials.set(saved.id, secret); else if (!remember) credentials.delete(saved.id);
  // Remembering is a stored choice for this connection: unchecking forbids the
  // connect-time write-back, checking it again lifts the refusal.
  if (needsSecret) { if (remember) declinedCredentials.delete(saved.id); else declinedCredentials.add(saved.id); }
  $('#connection-dialog').close(); await loadProfiles(); if (isNew) await connect(saved.id); else { renderSessionInfo(); toast('连接配置已保存，下次连接时生效'); }
});
$('#reset-host-key').onclick = safe(async () => { const id = $('#connection-form').elements.id.value; if (!id || !await ask({ title: '重置主机指纹？', description: '仅在确认服务器重装或主机密钥正常更换后操作。清除后，下次连接仍需核实并确认新指纹。', confirm: '重置指纹' })) return; await post(`/api/profiles/${id}/reset-key`, {}); toast('已清除记录，下次连接需要确认指纹'); });
$('#new-group').onclick = () => openServerGroupEditor();
document.addEventListener('keydown', event => { if (event.key === 'Escape' && !document.querySelector('dialog[open]')) setDrawer(false); if (event.key === 'Tab' && !$('#connections-drawer').hidden && !document.querySelector('dialog[open]')) { const controls = [...$('#connections-drawer').querySelectorAll('button,input')].filter(el => !el.disabled && el.getClientRects().length); if (event.shiftKey && document.activeElement === controls[0]) { event.preventDefault(); controls.at(-1).focus(); } else if (!event.shiftKey && document.activeElement === controls.at(-1)) { event.preventDefault(); controls[0].focus(); } } });

// Stream files through SFTP. Completion reflects remote writes, not just browser upload progress.
function setWorkspaceVisible(visible, persist = true) {
  if (persist) workspaceInitialized = true;
  $('#files-panel').hidden = !visible; $('#files-splitter').hidden = !visible;
  $('#workspace').classList.toggle('files-hidden', !visible);
  $('#toggle-sftp').setAttribute('aria-expanded', String(visible));
  if (persist) { layout.filesHidden = !visible; save('cloudshell.layout', layout); }
  syncRightPanelWindow();
  requestAnimationFrame(applyUIScale);
  // Re-evaluate the shell indicator immediately when the SFTP pane is hidden
  // or restored; otherwise a completion viewed in the pane could stay hidden
  // after the user returns to the terminal.
  updateTransferNotice();
}
// Serialize native resize requests, including placement changes and rapid toggles.
let rightPanelOpen = false, rightPanelResize = Promise.resolve();
function syncRightPanelWindow() {
  const open = appearance.filesPosition === 'right' && !$('#files-panel').hidden;
  if (open === rightPanelOpen) return;
  rightPanelOpen = open;
  const desktop = native();
  if (!desktop?.SetFilesPanel) return;
  const width = Math.round(428 * effectiveScale);
  rightPanelResize = rightPanelResize.then(() => desktop.SetFilesPanel(open, width))
    .catch(error => { console.warn('无法调整 SFTP 窗口宽度', error); });
}
$('#toggle-sftp').onclick = () => {
  if ($('#files-panel').hidden) { showPane('files'); setWorkspaceVisible(true); }
  else setWorkspaceVisible(false);
};
// The panel's own button only closes the SFTP panel; placement lives in settings.
$('#close-files-panel').onclick = () => setWorkspaceVisible(false);
function showPane(pane, persist = true) { workspaceInitialized = true; setWorkspaceVisible(true, false); $$('.file-tab').forEach(tab => tab.classList.toggle('active', tab.dataset.pane === pane)); $('#files-view').hidden = pane !== 'files'; $('#transfers-view').hidden = pane !== 'transfers'; $('#commands-view').hidden = pane !== 'commands'; $('#common-apps-view').hidden = pane !== 'common-apps'; window.DengCommonApps?.render(); $('.files-tip').hidden = pane !== 'files'; if (persist) save('dengshell.workspace', { ...readSaved('dengshell.workspace', {}), pane }); updateTransferNotice(); }
$$('.file-tab').forEach(tab => { tab.onclick = () => showPane(tab.dataset.pane); });
$('#choose-files').onclick = event => { const menu = $('#upload-menu'), rect = event.currentTarget.getBoundingClientRect(); menu.hidden = !menu.hidden; menu.style.left = `${Math.min(rect.left / effectiveScale, logicalWidth() - 155)}px`; menu.style.top = `${Math.min(rect.bottom / effectiveScale + 5, logicalHeight() - 90)}px`; };
document.addEventListener('click', event => { if (!event.target.closest('#upload-menu, #choose-files')) $('#upload-menu').hidden = true; });
$('#upload-files-option').onclick = safe(() => { $('#upload-menu').hidden = true; return chooseUploads(); });
// Shared with the lrzsz bridge: choose local files and upload them to a given
// remote directory. The browser fallback picker always targets the active
// session, so a background session cannot silently receive the files.
async function chooseUploads(state = current(), directory = state?.cwd) {
 if (!state?.connected) return toast('请先连接服务器');
 if (native()) { const paths = await native().ChooseUploads(); if (paths?.length) await uploadNative(paths, state, directory); }
 else if (state === current()) { if (directory !== state.cwd && state.sftpAvailable !== false) { try { await navigate(directory, state); } catch {} } $('#file-picker').click(); }
 else toast('请在该连接对应的窗口里用文件面板上传');
}
$('#upload-folder-option').onclick = safe(async () => { $('#upload-menu').hidden = true; if (native()) { const state = current(); const directory = state?.cwd; const path = await native().ChooseFolder(); if (path) await uploadNative([path], state, directory); } else $('#folder-picker').click(); });
for (const id of ['file-picker', 'folder-picker']) $(`#${id}`).onchange = safe(async event => { const files = [...event.target.files]; event.target.value = ''; await queueFiles(files.map(file => ({ file, relativePath: file.webkitRelativePath || file.name }))); });
let activeUploads = 0;
async function queueFiles(items, state = current(), destination = state?.cwd) {
  if (state?.sftpAvailable === false) return toast('当前连接的 SFTP 文件服务不可用');
  if (!state?.connected) return toast('请先连接服务器');
  if (!items.length) return toast('没有可上传的文件');
  const prepared = items.map(({ file, relativePath }) => ({ file, target: normalizePath(relativePath, destination) }));
  const decision = await window.DengUploadConfirmation.prepare(state, { targets: prepared.map(item => item.target), directory: destination });
  if (!decision) return;
  const approved = new Set(decision.overwriteTargets), skipped = new Set(decision.skipTargets);
  const selected = prepared.filter(item => !window.DengUploadConfirmation.skipped(item.target, skipped));
  if (!selected.length) return toast('已跳过全部同名文件');
  for (const { file, target } of selected) { const id = crypto.randomUUID(); localTasks.set(id, { id, sessionId: state.id, name: file.name, target, total: file.size, done: 0, status: 'queued', file, overwrite: approved.has(target) }); }
  renderTransfers(); pumpUploads(); toast(`已添加 ${selected.length} 个上传任务`);
}
function pumpUploads() { for (const task of localTasks.values()) if (task.file && !task.xhr && task.status === 'queued' && activeUploads < 4) { activeUploads++; task.status = 'uploading'; uploadBrowser(task).finally(() => { activeUploads--; pumpUploads(); renderTransfers(); }); } }
async function uploadBrowser(task) {
  try {
    if (!sessions.get(task.sessionId)?.connected) throw new Error('连接已断开，请重新连接后再次选择文件');
    await new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest(); task.xhr = xhr;
      xhr.open('POST', endpoint(`/api/sessions/${task.sessionId}/upload?path=${encodeURIComponent(task.target)}&task=${encodeURIComponent(task.id)}&overwrite=${task.overwrite ? '1' : '0'}`)); xhr.setRequestHeader('X-CloudShell-Token', boot.token); xhr.setRequestHeader('Content-Type', 'application/octet-stream');
      xhr.onload = () => { if (xhr.status >= 200 && xhr.status < 300) resolve(); else { let message = '上传失败'; try { message = JSON.parse(xhr.responseText).error; } catch {} reject(new Error(message)); } };
      xhr.onerror = () => reject(new Error('连接中断，文件未上传完成')); xhr.onabort = () => reject(new Error('已取消')); xhr.send(task.file);
    }); task.status = 'done'; task.done = task.total; refreshUploaded(task);
  } catch (error) { task.status = error.message === '已取消' ? 'cancelled' : 'failed'; task.error = error.message; }
  finally { task.xhr = null; renderTransfers(); }
}
async function uploadNative(paths, state = current(), directory = state?.cwd) {
  if (!state?.connected) return toast('请先连接服务器');
  if (state.sftpAvailable === false) return toast('当前连接的 SFTP 文件服务不可用');
  const decision = await window.DengUploadConfirmation.prepare(state, { paths, directory });
  if (!decision) return;
  const result = await post(`/api/sessions/${state.id}/upload-local`, { paths, directory, ...decision });
  for (const id of result.ids) { nativeUploadIDs.add(id); const task = localTasks.get(id); if (task?.historicalResult) { task.resultSeen = false; delete task.historicalResult; } }
  if (result.ids.length) toast(`已添加 ${result.ids.length} 个上传任务`); await pollTransfers(); updateTransferNotice(); if (!result.ids.length) { toast(decision.skipTargets.length ? '已跳过同名项目' : '目录已创建'); await navigate(state.cwd, state); }
}
window.cloudshellNativeDrop = safe(async ({ x, y, paths }) => {
  const rect = $('#files-panel').getBoundingClientRect(); if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) return toast('请将文件拖入下方文件区域');
  if (paths?.length) await uploadNative(paths);
});
function refreshUploaded(task) { const state = sessions.get(task.sessionId); if (!state?.connected || task.refreshed) return; task.refreshed = true; if (task.target.startsWith(state.cwd === '/' ? '/' : state.cwd + '/')) navigate(state.cwd, state).catch(() => {}); }
let transfersBusy = false, transferEpoch = 0, clearingTransfers = false, transfersInitialized = false;
const nativeUploadIDs = new Set();
async function pollTransfers() {
  if (transfersBusy) return; transfersBusy = true; const epoch = transferEpoch;
  try {
    const tasks = await api('/api/transfers'); if (epoch !== transferEpoch) return;
    const present = new Set(tasks.map(task => task.id));
    for (const [id, task] of localTasks) if (task.backendSeen && task.status === 'done' && !present.has(id)) localTasks.delete(id);
    for (const incoming of tasks) {
      let task = localTasks.get(incoming.id);
      if (task) {
        if (!['done', 'failed', 'cancelled'].includes(task.status) || incoming.status !== 'uploading') Object.assign(task, incoming);
      } else {
        const historical = !transfersInitialized && !nativeUploadIDs.has(incoming.id) && ['done', 'failed', 'cancelled'].includes(incoming.status);
        task = { ...incoming, resultSeen: historical, historicalResult: historical }; localTasks.set(task.id, task);
      }
      nativeUploadIDs.delete(task.id); task.backendSeen = true;
      if (task.status === 'done') refreshUploaded(task);
    }
    transfersInitialized = true; renderTransfers();
  } catch {} finally { transfersBusy = false; }
}
setInterval(pollTransfers, 1000);
async function clearCompletedTransfers() {
  if (clearingTransfers) return;
  clearingTransfers = true; transferEpoch++; renderTransfers();
  const localDone = [...localTasks.values()].filter(t => t.status === 'done').map(t => t.id);
  try {
    const result = await post('/api/transfers/clear-completed', {});
    for (const id of new Set([...result.ids, ...localDone])) {
      if (localTasks.get(id)?.status === 'done') localTasks.delete(id);
    }
  } finally { transferEpoch++; clearingTransfers = false; renderTransfers(); }
}
$('#clear-completed-transfers').onclick = safe(clearCompletedTransfers);
async function cancelTask(task) {
  if (task.status === 'queued') { if (task.xhr || !task.file) { task.xhr?.abort(); await remove(`/api/transfers/${task.id}`); } task.status = 'cancelled'; }
  else if (task.status === 'uploading') { task.xhr?.abort(); await remove(`/api/transfers/${task.id}`); }
  else { await remove(`/api/transfers/${task.id}`); localTasks.delete(task.id); }
  renderTransfers();
}
async function retryTask(task) {
  const overwrite = task.error?.includes('已存在'); if (overwrite && !await ask({ title: `覆盖 ${task.name}？`, description: `将替换远程文件 ${task.target}。`, confirm: '覆盖并上传' })) return;
  if (!task.file) { await post(`/api/transfers/${task.id}/retry`, { overwrite }); await remove(`/api/transfers/${task.id}`); localTasks.delete(task.id); await pollTransfers(); return; }
  await remove(`/api/transfers/${task.id}`); localTasks.delete(task.id); const next = { ...task, id: crypto.randomUUID(), done: 0, error: '', status: 'queued', overwrite, refreshed: false, resultSeen: false, historicalResult: false, completionNotified: false }; localTasks.set(next.id, next); pumpUploads(); renderTransfers();
}
// Read state lives on each task so repeated polls cannot re-notify a result.
// Initial history is already read; jobs created by this window are always new.
function updateTransferNotice() {
  const viewing = !document.hidden && !window.DevShellWindowHidden && !window.DengSecurityLock?.isLocked()
    && !$('#files-panel').hidden && !$('#transfers-view').hidden && $('#transfers-view').getClientRects().length > 0
    && !document.querySelector('dialog[open]');
  const sessionID = current()?.id || '';
  const results = [...localTasks.values()].filter(task => ['done', 'failed'].includes(task.status)
    && (!sessionID || !task.sessionId || task.sessionId === sessionID));
  window.DevShellTransfers?.update([...localTasks.values()], viewing, sessionID);
  if (viewing) for (const task of results) { task.resultSeen = true; delete task.historicalResult; }
  const unread = results.filter(task => !task.resultSeen), failed = unread.filter(task => task.status === 'failed').length;
  const badge = $('#transfer-count'), tab = $('.file-tab[data-pane="transfers"]');
  badge.hidden = unread.length === 0; badge.textContent = unread.length > 99 ? '99+' : String(unread.length);
  const label = unread.length ? `传输任务：${unread.length} 个新结果（完成 ${unread.length - failed} 项${failed ? `，失败 ${failed} 项` : ''}）` : '传输任务';
  tab.title = label; tab.setAttribute('aria-label', label); badge.setAttribute('aria-label', label);
}
function renderTransfers() {
  $('#clear-completed-transfers').disabled = clearingTransfers || ![...localTasks.values()].some(t => t.status === 'done');
  updateTransferNotice(); $('#transfer-empty').hidden = localTasks.size > 0;
  const label = { queued: '等待上传', uploading: '上传中', done: '已完成', failed: '失败', cancelled: '已取消' };
  $('#transfer-list').replaceChildren(...[...localTasks.values()].map(task => {
    const row = node('div', 'transfer-row'), info = node('div', 'transfer-info'); const name = node('strong', '', task.name), detail = node('small', '', task.error || task.target); name.title = task.name; detail.title = task.error || task.target; info.append(name, detail);
    const percent = task.status === 'done' ? 100 : task.total > 0 ? Math.max(0, Math.min(100, Math.round((task.done || 0) / task.total * 100))) : 0;
    const progress = node('div', `transfer-progress ${task.status}`);
    progress.setAttribute('role', 'progressbar'); progress.setAttribute('aria-label', task.name + ' 上传进度');
    progress.setAttribute('aria-valuemin', '0'); progress.setAttribute('aria-valuemax', '100'); progress.setAttribute('aria-valuenow', String(percent));
    const ring = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); ring.setAttribute('viewBox', '0 0 48 48'); ring.setAttribute('aria-hidden', 'true');
    for (const type of ['track', 'fill']) {
      const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
      circle.setAttribute('class', `transfer-ring-${type}`); circle.setAttribute('cx', '24'); circle.setAttribute('cy', '24'); circle.setAttribute('r', '20'); circle.setAttribute('pathLength', '100');
      if (type === 'fill') circle.setAttribute('stroke-dasharray', `${percent} 100`);
      ring.append(circle);
    }
    progress.append(ring, node('span', 'transfer-percent', `${percent}%`));
    const status = node('span', `transfer-state ${task.status}`, label[task.status] || task.status);
    const meta = node('div', 'transfer-meta'); meta.append(node('span', 'transfer-size', prettySize(task.done) + ' / ' + prettySize(task.total)), status); info.append(meta);
    const actions = node('div', 'transfer-actions');
    const cancel = node('button', 'icon-button'); cancel.title = ['queued', 'uploading'].includes(task.status) ? '取消上传' : '移除记录'; cancel.setAttribute('aria-label', cancel.title); cancel.append(icon('close')); cancel.onclick = safe(() => cancelTask(task));
    row.append(progress, info, actions);
    if (['failed', 'cancelled'].includes(task.status) && (task.file || task.retryable)) { const retry = node('button', 'transfer-retry', task.error?.includes('已存在') ? '覆盖' : '重试'); retry.onclick = safe(() => retryTask(task)); actions.append(retry); }
    actions.append(cancel); return row;
  }));
}
const filesPanel = $('#files-panel'); let dragDepth = 0;
const isFileDrag = event => [...(event.dataTransfer?.types || [])].includes('Files');
window.addEventListener('dragover', event => { if (isFileDrag(event)) { event.preventDefault(); event.dataTransfer.dropEffect = filesPanel.contains(event.target) && current()?.connected ? 'copy' : 'none'; } });
window.addEventListener('drop', event => { if (isFileDrag(event)) event.preventDefault(); dragDepth = 0; $('#drop-overlay').hidden = true; });
filesPanel.addEventListener('dragenter', event => { if (!isFileDrag(event) || !current()?.connected || native()) return; event.preventDefault(); dragDepth++; $('#drop-overlay').hidden = false; });
filesPanel.addEventListener('dragleave', () => { dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) $('#drop-overlay').hidden = true; });
async function readEntry(entry, prefix = '') {
  if (entry.isFile) { const file = await new Promise((resolve, reject) => entry.file(resolve, reject)); return [{ file, relativePath: prefix + file.name }]; }
  if (!entry.isDirectory) return []; const reader = entry.createReader(), result = [];
  while (true) { const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject)); if (!batch.length) break; for (const child of batch) result.push(...await readEntry(child, prefix + entry.name + '/')); }
  return result;
}
filesPanel.addEventListener('drop', safe(async event => {
  if (!isFileDrag(event) || native()) return; event.preventDefault(); const state = current(), directory = state?.cwd, files = [...event.dataTransfer.files], entries = [...event.dataTransfer.items].filter(item => item.kind === 'file').map(item => item.webkitGetAsEntry?.());
  dragDepth = 0; $('#drop-overlay').hidden = true;
  if (entries.length && entries.every(Boolean)) { const items = []; for (const entry of entries) items.push(...await readEntry(entry)); await queueFiles(items, state, directory); }
  else await queueFiles(files.map(file => ({ file, relativePath: file.name })), state, directory);
}));

const savedLayout = readSaved('cloudshell.layout', {});
let layout = savedLayout && typeof savedLayout === 'object' ? savedLayout : {};
// Adopt the roomier terminal once; subsequent manual pane sizes stay intact.
if (layout.version !== 2) { delete layout.files; layout.version = 2; save('cloudshell.layout', layout); }
// A missing visibility preference means this is the first SFTP-panel choice.
// Start closed; an explicit saved false still restores the panel as open.
setWorkspaceVisible(layout.filesHidden === false, false);
function filePanelLimits() {
  const workspace = $('#workspace');
  // The separator is 8px in either direction; its full height in right mode
  // must never overwrite the separately remembered bottom-panel height.
  const maxWidth = Math.max(160, workspace.clientWidth - 240 - 8);
  return { maxWidth, maxHeight: Math.max(100, workspace.clientHeight - 160 - 8) };
}
function clampLayouts() {
  const root = document.documentElement;
  // Fixed side panels leave window growth to the terminal. Old saved widths
  // must not override the defaults; only constrain SFTP in a small window.
  root.style.setProperty('--sidebar-width', '304px');
  const { maxWidth, maxHeight } = filePanelLimits();
  root.style.setProperty('--files-width', `${Math.min(420, maxWidth)}px`);
  // Top/bottom SFTP heights remain independently resizable.
  if (Number.isFinite(layout.files)) root.style.setProperty('--files-height', `${Math.max(100, Math.min(maxHeight, layout.files))}px`);
  const right = appearance.filesPosition === 'right', splitter = $('#files-splitter');
  if (!right) {
    splitter.setAttribute('aria-valuemin', 100);
    splitter.setAttribute('aria-valuemax', maxHeight);
    splitter.setAttribute('aria-valuenow', Math.round($('#files-panel').clientHeight));
  }
}
function resizeFiles(event) {
  if (appearance.filesPosition === 'right') return;
  const rect = $('#workspace').getBoundingClientRect(), { maxHeight } = filePanelLimits();
  layout.files = Math.max(100, Math.min(maxHeight, (appearance.filesPosition === 'top' ? event.clientY - rect.top : rect.bottom - event.clientY) / effectiveScale));
  clampLayouts();
}
function bindSplitter(selector, handler, keyboardHandler) {
  const splitter = $(selector);
  let dragging = false;
  splitter.addEventListener('pointerdown', event => {
    if (event.button !== 0 || splitter.getAttribute('aria-disabled') === 'true') return;
    dragging = true;
    splitter.setPointerCapture(event.pointerId);
    document.body.style.userSelect = 'none';
    document.body.style.cursor = getComputedStyle(splitter).cursor;
  });
  splitter.addEventListener('pointermove', event => { if (dragging) handler(event); });
  const stop = () => { if (!dragging) return; dragging = false; document.body.style.userSelect = ''; document.body.style.cursor = ''; save('cloudshell.layout', layout); };
  splitter.addEventListener('pointerup', stop);
  splitter.addEventListener('pointercancel', stop);
  splitter.addEventListener('lostpointercapture', stop);
  splitter.addEventListener('keydown', event => {
    if (splitter.getAttribute('aria-disabled') === 'true') return;
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
    event.preventDefault(); keyboardHandler(event.key); save('cloudshell.layout', layout);
  });
}
bindSplitter('#files-splitter', resizeFiles, key => {
  if (appearance.filesPosition === 'right') return;
  const { maxHeight } = filePanelLimits();
  if (key === 'ArrowUp' || key === 'ArrowDown') {
    layout.files = Math.max(100, Math.min(maxHeight, $('#files-panel').clientHeight + (key === 'ArrowUp' ? 20 : -20) * (appearance.filesPosition === 'top' ? -1 : 1)));
  }
  clampLayouts();
});
$('#reset-layout').onclick = () => {
  layout = { version: 2 };
  save('cloudshell.layout', layout);
  document.documentElement.style.removeProperty('--sidebar-width');
  document.documentElement.style.removeProperty('--files-height');
  document.documentElement.style.removeProperty('--files-width');
  document.documentElement.style.removeProperty('--terminal-font');
  terminalFont = 14; changeFont(0);
  clampLayouts();
  chooseAppearance({ monitorVisible: true, filesPosition: 'right' }).catch(error => toast(error.message));
  toast('已恢复默认布局与终端字号');
};
window.addEventListener('resize', () => {
  clampLayouts();
  requestAnimationFrame(() => {
    fitActive();
  });
});

initializeWorkspaceTools(); initializeAppearance(); initializeRefinements(); renderSessionInfo(); renderFiles(); renderTransfers(); applyUIScale();
let windowsDropReady = false;
function initializeWindowsDrop() {
  if (!windowsDropReady && window.chrome?.webview?.postMessageWithAdditionalObjects && window.runtime?.OnFileDrop) {
    // Register WebView2's file-path resolver. Go handles the resolved drop once.
    window.runtime.OnFileDrop(() => {}, false);
    windowsDropReady = true;
  }
}
async function initializeConfig() {
  try { if (desktopPage) await waitForDesktop(); await initializeWindowControls(); await initializeQuitConfirmation(); await window.DengSecurityLock?.ready; if(window.DengSecurityLock?.isLocked()) window.runtime?.EventsEmit('cloudshell:ready'); await window.DengSecurityLock?.whenUnlocked(); window.DengSessionWindows.prepareRestore(); await loadProfiles(); await window.DengSessionWindows.restore(); initializeWindowsDrop(); syncDesktopTheme(); $('#connection-button').classList.remove('failed'); $('#connection-button').title = '打开服务器管理'; window.runtime?.EventsEmit('cloudshell:ready'); }
  catch (error) { $('#connection-button').classList.add('failed'); $('#connection-button').title = error.message + ' · 点击重试'; toast(error.message); }
}
$('#connection-button').onclick = () => $('#connection-button').classList.contains('failed') ? initializeConfig() : setDrawer(true);
initializeConfig();
document.fonts.ready.then(fitActive);
window.addEventListener('pagehide', () => { for (const state of sessions.values()) state.ws?.close(); });
for (const pane of $$('.sidebar, .file-content, .transfers-view, .connections-drawer, .commands-content')) {
  let idleTimer;
  pane.addEventListener('scroll', () => { pane.classList.add('is-scrolling'); clearTimeout(idleTimer); idleTimer = setTimeout(() => pane.classList.remove('is-scrolling'), 800); }, { passive: true });
}
