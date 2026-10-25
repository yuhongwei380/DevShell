import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';

// The completion module only touches the DOM through init() and the keydown
// handler, so a tiny element stub is enough to drive real Tab presses.
function element(tag, className = '', text = '') {
  return {
    tag, className, textContent: text, hidden: false, isConnected: true,
    children: [], listeners: {}, attrs: {}, dataset: {},
    append(...items) { this.children.push(...items); },
    replaceChildren(...items) { this.children = items; },
    setAttribute(name, value) { this.attrs[name] = String(value); },
    removeAttribute(name) { delete this.attrs[name]; },
    addEventListener(type, handler) { (this.listeners[type] ||= []).push(handler); },
    classList: { toggle() {} },
    scrollIntoView() {},
    focus() {},
  };
}
const area = element('textarea', '', '');
area.value = ''; area.selectionStart = 0; area.selectionEnd = 0;
area.setRangeText = function (text, start, end) {
  this.value = this.value.slice(0, start) + text + this.value.slice(end);
  this.selectionStart = this.selectionEnd = start + text.length;
};
const bar = element('div', 'terminal-command-bar');
const documentListeners = {};
let activeState = null;
const context = vm.createContext({
  window: { addEventListener() {} },
  document: {
    addEventListener(type, handler) { (documentListeners[type] ||= []).push(handler); },
    getElementById: id => id === 'command-input' ? area : null,
    querySelector: selector => selector === '.terminal-command-bar' ? bar : null,
  },
  node: element, current: () => activeState, resizeCommandInput() {},
});
vm.runInContext(fs.readFileSync(new URL('../../web/js/command-completion.js', import.meta.url), 'utf8'), context);
const api = context.window.DengCommandCompletion;
documentListeners.DOMContentLoaded.forEach(handler => handler());

const press = (name, options = {}) => {
  const event = { key: name, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, isComposing: false, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, target: area, ...options };
  for (const handler of area.listeners.keydown || []) handler(event);
  return event;
};
// Typing dispatches the input listener too, exactly like a real keystroke: the
// candidate session must restart after any edit.
const type = text => {
  area.value = text; area.selectionStart = area.selectionEnd = text.length;
  for (const handler of area.listeners.input || []) handler();
};
const popup = () => bar.children[0];
const rows = () => popup()?.children.map(row => `${row.children[0].textContent} ${row.children[1].textContent}`.trim()) || [];

// The module runs in its own realm, so compare via JSON to skip cross-realm
// prototype identity checks.
const plain = value => JSON.parse(JSON.stringify(value));
// Welcome / start page: no reconnect, exit or help — there is no tab to act on.
assert.deepEqual(plain(api.commandsFor(null).map(c => c.name)), ['ssh', 'ping', 'traceroute', 'nslookup', 'dig', 'curl'], 'welcome set');
// A tab whose shell exited: the full local prompt vocabulary.
assert.deepEqual(plain(api.commandsFor({}).map(c => c.name)), ['ssh', 'reconnect', 'ping', 'traceroute', 'nslookup', 'dig', 'curl', 'help', 'exit'], 'session set');
for (const busy of [{ connected: true }, { pendingConnection: true }, { disconnecting: true }, { detaching: true }, { handoffProvisional: true }, { ownershipUncertain: true }])
  assert.equal(api.commandsFor(busy), null, `connected or transient ${Object.keys(busy)[0]} must not complete`);

// Only the first token of the line is a command word; arguments stay untouched.
assert.deepEqual(plain(api.wordAt('ping', 4)), { word: 'ping', start: 0, end: 4 });
assert.equal(api.wordAt('ssh pi', 6), null, 'the second token is an argument');
assert.deepEqual(plain(api.wordAt('foo\npi', 6)), { word: 'pi', start: 4, end: 6 }, 'a command word may start on any line');
assert.deepEqual(plain(api.wordAt('pin', 2)), { word: 'pi', start: 0, end: 3 }, 'caret mid-word consumes the tail');
assert.equal(api.commonPrefix(['ssh', 'ping']), '');
assert.equal(api.commonPrefix(['nslookup', 'nothing']), 'n');

// Unique match completes in one Tab press and shows the usage row.
type('pi');
assert.equal(press('Tab').defaultPrevented, true, 'Tab is captured when it can complete');
assert.equal(area.value, 'ping', 'unique completion');
assert.equal(popup().hidden, false, 'the candidate box shows usage hints');
assert.deepEqual(plain(rows()), ['ping 主机 [-t]']);

// Empty input opens every candidate first, then Tab cycles through them.
type('');
press('Tab');
assert.equal(area.value, '', 'no common prefix among all commands');
assert.equal(rows().length, 6, 'welcome page offers six commands');
press('Tab'); assert.equal(area.value, 'ssh', 'first candidate');
press('Tab'); assert.equal(area.value, 'ping', 'second candidate');
press('Tab'); assert.equal(area.value, 'traceroute', 'third candidate');
press('Tab', { shiftKey: true }); assert.equal(area.value, 'ping', 'Shift+Tab cycles backwards');
press('Tab', { shiftKey: true }); assert.equal(area.value, 'ssh');
press('Tab', { shiftKey: true }); assert.equal(area.value, 'curl', 'wraps below the first entry');

// Any edit restarts the session; Escape and history arrows close the box.
type('t');
assert.equal(press('Tab').defaultPrevented, true);
assert.equal(area.value, 'traceroute', 'unique match in the welcome set');
press('Escape');
assert.equal(popup().hidden, true, 'Escape closes the candidate box');
type('n');
press('Tab');
assert.equal(area.value, 'nslookup');
press('ArrowUp');
assert.equal(popup().hidden, true, 'history recall closes the box');

// Arguments never complete and Tab behaves as before: it inserts a character.
type('ssh pi');
assert.equal(press('Tab').defaultPrevented, false, 'argument position keeps plain Tab');
assert.equal(area.value, 'ssh pi', 'nothing rewritten');
type('zz');
assert.equal(press('Tab').defaultPrevented, false, 'no candidate for an unknown word');

// A disconnected tab also completes exit / reconnect, case-insensitively.
activeState = {};
type('e');
press('Tab'); assert.equal(area.value, 'exit');
type('RE');
press('Tab'); assert.equal(area.value, 'reconnect', 'completion lowercases to the command name');
// Mid-word caret: the partial tail is replaced, not left behind as "reconnect".
area.value = 'rec'; area.selectionStart = area.selectionEnd = 2;
press('Tab');
assert.equal(area.value, 'reconnect', 'caret inside the word consumes its tail');

// A connected session never sees completion: the remote shell owns Tab.
activeState = { connected: true, ready: true };
type('pi');
assert.equal(press('Tab').defaultPrevented, false);
assert.equal(area.value, 'pi', 'connected input is untouched');

console.log('PASS: welcome vs session command sets; unique completion with usage popup; cycle and Shift+Tab wrap; argument, unknown-word and connected passthrough; mid-word and case handling; Escape/arrows close the box.');
