import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

// telnet 会话建立回归（vm 桩，无真实网络）：后端 TcpSessionInfo.host 是主机名，
// 曾经撞掉 createTerminal 建立的 state.host DOM 容器，导致 dataset.session 赋值
// 抛错、已建立的 TCP 连接被挂起、标签停在「正在连接」且关闭不发 DELETE。
const sessions = new Map(), temporaryProfiles = new Map(), posts = [], removed = [], sockets = [];
let uuids = 0;
const context = vm.createContext({
  crypto: { randomUUID: () => `uuid-${++uuids}` },
  AbortController,
  sessions, temporaryProfiles,
  makeSessionState: info => ({ ...info }),
  createTerminal: state => {
    state.host = { dataset: {}, hidden: true };
    state.term = { writeln() {}, options: {}, serialize: { serialize: () => '' } };
    state.openTerminalSocket = () => sockets.push(state.id);
  },
  activate() {}, renderTabs() {}, setDrawer() {},
  window: { DengStartTabs: null, DengRDP: { active: () => false }, DengTcpSession: undefined },
  nextSessionOrder: 1, activeID: null,
  post: async (path, body) => { posts.push({ path, body }); return { id: 'tcp-1', name: `telnet ${body.host}:${body.port}`, host: body.host, port: body.port, remote: `${body.host}:${body.port}` }; },
  remove: async path => { removed.push(path); return { ok: true }; },
  toast() {},
  preserveReconnectTerminal: async () => {}, restoreReconnectTerminal: async () => {},
  showConnectionProgress() {},
  current: () => null, renderSessionInfo() {}, updateStatus() {},
  connectionProfile: () => null, dropSessionView() {},
});
vm.runInContext(readFileSync(new URL('../../web/js/tcp-session.js', import.meta.url), 'utf8'), context);
const tcp = context.window.DengTcpSession;

const target = tcp.parse('telnet 198.51.100.7 443');
assert.equal(target.host, '198.51.100.7');
assert.equal(target.port, 443);
assert.equal(tcp.parse('telnet'), null);

const state = await tcp.open({ host: '198.51.100.7', port: 443 });
assert.ok(state, '拨号成功后返回会话');
assert.equal(posts[0].path, '/api/tcp-sessions');
// 关键断言：state.host 必须是终端 DOM 容器，而不是 info 里的主机名字符串。
assert.equal(typeof state.host, 'object', 'info.host 不得覆盖终端容器 state.host');
assert.equal(state.host.dataset.session, 'tcp-1');
assert.equal(sessions.get('tcp-1'), state);
assert.deepEqual(sockets, ['tcp-1'], '连接建立后终端中继挂上');

await tcp.close('tcp-1');
assert.deepEqual(removed, ['/api/tcp-sessions/tcp-1'], '关闭标签必须断开远端连接');

// 拨号失败：open 返回 null、不抛异常；伪配置保留以便重拨。
context.post = async () => { throw new Error('连接 203.0.113.9:23 失败：端口拒绝连接'); };
const failed = await tcp.open({ host: '203.0.113.9', port: 23 });
assert.equal(failed, null);
assert.equal(removed.length, 1, '失败的拨号没有建立连接，也不需要 DELETE');
console.log('TCP 会话：host 容器不被主机名覆盖、中继挂载、关闭断连与失败回落全部通过。');
