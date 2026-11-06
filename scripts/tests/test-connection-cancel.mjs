import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { webcrypto } from 'node:crypto';

const appSource = readFileSync(new URL('../../web/js/app.js', import.meta.url), 'utf8');
const connectSource = appSource.slice(appSource.indexOf('async function connectProfile('), appSource.indexOf('\nfunction showConnectionProgress('));
const releaseSource = appSource.slice(appSource.indexOf('function releaseConnectionAttempt('), appSource.indexOf('\nasync function connect('));
const handlerSource = appSource.slice(appSource.indexOf('terminal.attachCustomKeyEventHandler('), appSource.indexOf('terminal.parser.registerOscHandler'));
for (const [name, source] of [['connectProfile', connectSource], ['releaseConnectionAttempt', releaseSource], ['key handler', handlerSource]]) assert.ok(source.length > 0, `${name} slice missing; update this test`);
const tick = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };

// Ctrl+C in a connecting tab must cancel the attempt; connected tabs keep \x03.
let handler;
const inputs = [], cancels = [];
let copies = 0;
const keyState = { pendingConnection: true };
const keyContext = {
  terminal: { attachCustomKeyEventHandler(fn) { handler = fn; }, clearSelection() {}, scrollToBottom() {} },
  state: keyState,
  sendInput: (_state, data) => inputs.push(data),
  cancelPendingConnection: state => cancels.push(state),
  copyTerminalSelection: async () => { copies++; },
  toast() {},
};
vm.createContext(keyContext);
vm.runInContext(handlerSource, keyContext);
const key = extra => ({ type: 'keydown', ctrlKey: true, altKey: false, shiftKey: false, code: 'KeyC', preventDefault() {}, ...extra });
assert.equal(handler(key()), false);
assert.deepEqual(cancels, [keyState], 'Ctrl+C while connecting must cancel the attempt');
assert.deepEqual(inputs, [], 'connecting Ctrl+C must not send \\x03 to the pending session');
keyState.pendingConnection = false;
handler(key());
assert.deepEqual(inputs, ['\x03'], 'connected Ctrl+C keeps sending the interrupt');
assert.equal(handler(key({ shiftKey: true })), false);
assert.equal(copies, 1, 'Ctrl+Shift+C stays a copy shortcut');
assert.equal(cancels.length, 1);
assert.equal(handler({ type: 'keyup', ctrlKey: true, code: 'KeyC' }), true);

function baseContext(profile, { api, ask } = {}) {
  const context = {
    profiles: [profile], managedKeys: [],
    sessions: new Map(), connecting: new Set(), connectionAttempts: new Map(), connectionRequests: new Map(), credentials: new Map(),
    temporaryProfiles: new Set(), declinedCredentials: new Set(),
    nextSessionOrder: 0, activeID: null, AbortController, crypto: webcrypto, window: {},
    connectionProfile: id => id === profile.id ? profile : undefined,
    makeSessionState: info => ({ ...info, host: { dataset: {} }, openTerminalSocket() {} }),
    current: () => null, createTerminal() {}, activate() {}, setDrawer() {}, renderTabs() {}, renderConnections() {}, renderSessionInfo() {},
    showConnectionForm() {}, toast() {}, updateStatus() {},
    showConnectionProgress: (_state, message) => context.messages.push(message),
    navigate: async () => {}, closeSession: async () => {},
    ask, api,
    remove: async path => { context.removed.push(path); return { ok: true }; },
    messages: [], removed: [],
  };
  vm.createContext(context);
  vm.runInContext(releaseSource + '\n' + connectSource, context);
  return context;
}
const passwordProfile = ({ hasSecret = false } = {}) => ({ id: 'profile', name: 'fixture', auth: 'password', host: 'h', port: 22, user: 'u', hasSecret });
const abortableAsk = options => new Promise(resolve => options.signal.addEventListener('abort', () => resolve(null)));

// Ctrl+C while the connect request is in flight (desktop IPC cannot abort it):
// the tab is released immediately and a late success is torn down afterwards.
let apiCalls = 0;
let resolveApi;
const lateRequest = new Promise(resolve => { resolveApi = resolve; });
const inflight = baseContext(passwordProfile({ hasSecret: true }), {
  api: () => { apiCalls++; return lateRequest; },
  ask: abortableAsk,
});
const inflightSession = inflight.connectProfile('profile', { background: false, requestKey: 'inflight' });
await tick();
const inflightState = [...inflight.sessions.values()][0];
assert.equal(inflightState.pendingConnection, true);
assert.equal(apiCalls, 1);
inflight.cancelPendingConnection(inflightState);
inflight.cancelPendingConnection(inflightState);
assert.equal(inflight.messages.filter(message => message.includes('已取消连接')).length, 1, 'repeat Ctrl+C must not pile up cancel notices');
assert.equal(await inflightSession, null, 'a cancelled attempt resolves to no session');
assert.match(inflight.messages.join('\n'), /已取消连接（Ctrl\+C）/);
assert.equal(inflightState.pendingConnection, false, 'the tab stays open as a local prompt after cancel');
assert.equal(inflight.connecting.size, 0);
assert.equal(inflight.connectionAttempts.size, 0);
assert.equal(inflight.messages.some(message => message.startsWith('连接主机失败')), false, 'cancel must not surface as a connection failure');
resolveApi({ id: 'late-session', home: '/fixture' });
await tick();
assert.deepEqual(inflight.removed, ['/api/sessions/late-session'], 'a session completed after the cancel must be removed');

// Closing a connecting tab aborts the same signal without the Ctrl+C helper:
// a session that still completes must be removed just the same.
let resolveClosed;
const closedRequest = new Promise(resolve => { resolveClosed = resolve; });
const closed = baseContext(passwordProfile({ hasSecret: true }), {
  api: () => closedRequest,
  ask: abortableAsk,
});
const closedSession = closed.connectProfile('profile', { background: false, requestKey: 'closing' });
await tick();
const closedState = [...closed.sessions.values()][0];
closedState.closed = true;
closedState.connectionAbort.abort();
assert.equal(await closedSession, null);
assert.equal(closed.messages.some(message => message.includes('已取消连接')), false, 'closing stays silent');
resolveClosed({ id: 'closed-late', home: '/fixture' });
await tick();
assert.deepEqual(closed.removed, ['/api/sessions/closed-late'], 'closing while connecting must also tear down late sessions');

// Ctrl+C while a credential prompt is waiting closes the prompt without dialing.
const prompting = baseContext(passwordProfile(), {
  api: () => { throw new Error('must not dial after cancel'); },
  ask: abortableAsk,
});
const promptingSession = prompting.connectProfile('profile', { background: false, requestKey: 'prompt' });
await tick();
const promptingState = [...prompting.sessions.values()][0];
assert.match(prompting.messages.at(-1), /等待输入凭据/);
prompting.cancelPendingConnection(promptingState);
assert.equal(await promptingSession, null);
assert.match(prompting.messages.join('\n'), /已取消连接/);
assert.equal(prompting.removed.length, 0);

console.log('PASS: Ctrl+C cancels in-flight connections and credential prompts, releases the tab, tears down late sessions, and keeps connected-tab interrupts and copy intact.');
