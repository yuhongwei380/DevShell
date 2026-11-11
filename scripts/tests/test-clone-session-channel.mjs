import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

// Xshell 语义前端回归：双击已连接标签默认「复制同服务器SSH会话」（复用同一条
// SSH 连接的新渠道标签，菜单项已按需求取消）；标签菜单「复制独立会话」克隆
// 「原名-clone」配置并立即拨号打开独立会话；收藏夹 chip 的「复制链接」生成并
// 自动收藏到源 chip 之后。会话菜单不再提供「新建同服务器会话」。
const appSource = readFileSync(new URL('../../web/js/app.js', import.meta.url), 'utf8');
const navSource = readFileSync(new URL('../../web/js/pinned-nav.js', import.meta.url), 'utf8');
const windowsSource = readFileSync(new URL('../../web/js/session-windows.js', import.meta.url), 'utf8');

const cloneSource = appSource.slice(appSource.indexOf('async function cloneSessionChannel('), appSource.indexOf('\nfunction writeTerminalAndWait('));
const linkSource = navSource.slice(navSource.indexOf('async function cloneLink('), navSource.indexOf('\n  // 右键菜单'));
for (const [name, source] of [['cloneSessionChannel', cloneSource], ['cloneLink', linkSource]]) assert.ok(source.startsWith(`async function ${name}`), `${name} slice missing; update this test`);

// 标签菜单与双击入口的静态契约。
assert.ok(!windowsSource.includes("'复制同服务器SSH会话'"), 'tab menu must no longer offer 复制同服务器SSH会话 (double-click keeps the channel copy)');
assert.ok(!windowsSource.includes('新建同服务器会话'), 'tab menu must no longer offer 新建同服务器会话');
assert.ok(windowsSource.includes('"复制独立会话"'), 'tab menu must offer 复制独立会话');
assert.ok(windowsSource.includes('await connect(saved.id)'), '复制独立会话 must open the copied session with a fresh connection');
assert.ok(!windowsSource.includes('复制链接'), 'tab menu must no longer offer 复制链接');
assert.ok(appSource.includes('if (!state.localShell && !state.telnet && !state.serial) safe(() => cloneSessionChannel(state))();'), 'double-click on a connected tab must default to opening a same-server session');
// 非占位标签在 createTerminal 内已自动接入终端；再调一次 openTerminalSocket 会
// 撞上后端 409「此会话已有终端」，新标签立刻变红并显示 DS-290（真实回归）。
assert.ok(cloneSource.includes('createTerminal(clone)'), 'the clone tab must create its terminal');
assert.ok(!cloneSource.includes('clone.openTerminalSocket('), 'cloneSessionChannel must not open the terminal socket a second time');

function cloneContext(post) {
  const context = {
    sessions: new Map(), nextSessionOrder: 4,
    post, encodeURIComponent,
    toast: message => context.messages.push(message),
    makeSessionState: info => ({ ...info, connected: true, ready: false, cwd: '/' }),
    // 真实 createTerminal 对非占位标签（无 localOnly/restoration）会赋值
    // openTerminalSocket 并立即接入一次；照此建模以捕捉任何二次接入。
    createTerminal: state => {
      context.created.push(state.id);
      state.openTerminalSocket = () => { context.socketed.push(state.id); };
      state.openTerminalSocket();
    },
    activate: id => { context.activated = id; },
    setDrawer() {}, renderConnections() {},
    refreshFileStatus: async state => { context.refreshed.push(state.id); },
    navigate: async () => {}, pollStats() {}, pollLatency() {},
    current: () => null,
    profileFor: state => ({ name: state.id === 'source' ? '源连接' : '源连接', verbose: 0 }),
    startVerboseDiagnostics() {},
    window: { DengCommandHistory: { refresh: async () => {} } },
    messages: [], created: [], socketed: [], refreshed: [], activated: null,
  };
  vm.createContext(context);
  vm.runInContext(cloneSource, context);
  return context;
}

// 未连接/非 SSH 标签不得触碰后端。
let posts = [];
let ctx = cloneContext(async (path, body) => { posts.push([path, body]); return { id: 'clone-1', profileId: 'same' }; });
assert.equal(await ctx.cloneSessionChannel({ id: 'source', connected: false }), null);
assert.equal(await ctx.cloneSessionChannel({ id: 'source', connected: true, serial: true }), null);
assert.deepEqual(posts, [], 'refused clones must not contact the backend');
assert.match(ctx.messages.join('\n'), /请先连接这个会话/);
assert.match(ctx.messages.join('\n'), /不是 SSH 会话/);

// 已连接的 SSH 标签：后端登记新会话 → 新标签打开独立终端渠道。
posts = [];
ctx = cloneContext(async (path, body) => { posts.push([path, body]); return { id: 'clone-1', profileId: 'same', sftpPending: true }; });
const cloned = await ctx.cloneSessionChannel({ id: 'source', connected: true, profileId: 'same' });
assert.equal(posts[0][0], '/api/sessions/source/channel', 'clone must ride the existing session endpoint');
assert.equal(JSON.stringify(posts[0][1]), '{}');
assert.equal(cloned.id, 'clone-1');
assert.equal(ctx.sessions.get('clone-1'), cloned);
assert.equal(cloned.announceConnectionReady, true);
assert.deepEqual(ctx.created, ['clone-1']);
assert.equal(ctx.activated, 'clone-1');
assert.deepEqual(ctx.socketed, ['clone-1'], 'the clone tab must attach its terminal channel exactly once');
assert.deepEqual(ctx.refreshed, ['clone-1'], 'pending SFTP is refreshed per clone session');
assert.match(ctx.messages.join(''), /已复制「源连接」的 SSH 连接/);

// 后端失败（如传输已断开）：只弹失败 toast，不留下空标签。
ctx = cloneContext(async () => { throw new Error('SSH 连接已断开，请重新连接'); });
assert.equal(await ctx.cloneSessionChannel({ id: 'source', connected: true }), null);
assert.equal(ctx.sessions.size, 0);
assert.match(ctx.messages.join(''), /复制同服务器SSH会话失败：SSH 连接已断开/);

function linkContext(post, pinned = ['source']) {
  const context = {
    profiles: [{ id: 'source', name: '源连接' }], pinned,
    post, encodeURIComponent,
    toast: message => context.messages.push(message),
    reorderSave: Promise.resolve(),
    readPinned() {}, writePinned: async () => { context.wrote = true; },
    renderConnections() {}, renderTabs() {}, renderSessionInfo() {}, render() {}, reflect() {},
    messages: [], wrote: false,
  };
  vm.createContext(context);
  vm.runInContext(linkSource, context);
  return context;
}

// 收藏夹 chip 的「复制链接」：默认名「原会话名-clone」，自动收藏到源 chip 右侧。
let linkPosts = [];
ctx = linkContext(async (path, body) => { linkPosts.push([path, body]); return { id: 'clone-id', name: '源连接-clone' }; }, ['other', 'source', 'third']);
const saved = await ctx.cloneLink(ctx.profiles[0], { pin: true });
assert.equal(linkPosts[0][0], '/api/profiles/source/duplicate');
assert.equal(linkPosts[0][1].name, '源连接-clone');
assert.equal(saved.id, 'clone-id');
assert.ok(ctx.profiles.some(entry => entry.id === 'clone-id'), 'clone must join the local profile list');
assert.deepEqual(ctx.pinned, ['other', 'source', 'clone-id', 'third'], 'chip clone pins right after the source');
assert.equal(ctx.wrote, true);
assert.match(ctx.messages.join(''), /已生成「源连接-clone」并加入收藏夹/);

// 标签菜单的「复制独立会话」用 cloneLink 克隆后再接 connect 打开独立会话。
ctx = linkContext(async () => ({ id: 'clone-id', name: '源连接-clone' }), ['source']);
await ctx.cloneLink(ctx.profiles[0]);
assert.deepEqual(ctx.pinned, ['source']);
assert.equal(ctx.wrote, false);

// 源连接不在收藏夹时，收藏的副本追加在末尾。
ctx = linkContext(async () => ({ id: 'clone-id', name: '源连接-clone' }), ['other']);
await ctx.cloneLink(ctx.profiles[0], { pin: true });
assert.deepEqual(ctx.pinned, ['other', 'clone-id']);

console.log('clone-session-channel frontend contract: ok');
