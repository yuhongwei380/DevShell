import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';

const [stage, chrome, modulePath] = process.argv.slice(2);
const { default: puppeteer } = await import(path.isAbsolute(modulePath) ? pathToFileURL(modulePath).href : modulePath);
const fixture = JSON.parse(fs.readFileSync(path.join(stage, 'browser-fixture.json')));
await fetch(new URL('/api/appearance/patch', fixture.url), { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': fixture.token }, body: JSON.stringify({ onboardingCompleted: true, uiScale: 1 }) });
const profileDir = fs.mkdtempSync(path.join(stage, 'edge-tabs-'));
const child = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking', '--remote-debugging-port=0', '--user-data-dir=' + profileDir, 'about:blank'], { stdio: 'ignore', windowsHide: true });
let browser, page;
const errors = [], checks = [];
const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
async function assertPlusFollowsLastTab() {
  await settle();
  const metrics = await page.evaluate(() => {
    const tabs = $('#session-tabs');
    const anchor = tabs.lastElementChild || $('#connection-button');
    const plus = $('#add-session').getBoundingClientRect(), previous = anchor.getBoundingClientRect();
    return { gap: plus.left - previous.right, top: plus.top, previousTop: previous.top };
  });
  assert(metrics.gap >= 0 && metrics.gap <= 8 && Math.abs(metrics.top - metrics.previousTop) < 5, JSON.stringify(metrics));
}
try {
  let endpoint;
  for (let i = 0; i < 100; i++) {
    try { const [port, suffix] = fs.readFileSync(path.join(profileDir, 'DevToolsActivePort'), 'utf8').trim().split(/\r?\n/); endpoint = `ws://127.0.0.1:${port}${suffix}`; break; } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert(endpoint, 'browser started');
  browser = await puppeteer.connect({ browserWSEndpoint: endpoint });
  page = await browser.newPage(); page.on('pageerror', error => errors.push(error.message));
  await page.setViewport({ width: 1440, height: 1000 });
  await page.goto(fixture.url, { waitUntil: 'networkidle0' });
  await page.waitForFunction(() => window.DengStartTabs && window.DengQuickConnect);
  await page.evaluate(() => reflectWindowState({ frameless: true }));
  assert.equal(await page.$eval('#about-button', el => el.nextElementSibling.id), 'settings-button');
  assert.equal(await page.$eval('#settings-button', el => el.nextElementSibling.id), 'github-button');
  assert.equal(await page.$eval('#settings-button use', el => el.getAttribute('href')), '#i-settings');
  await page.click('#settings-button');
  await page.waitForSelector('#settings-menu:not([hidden])');
  assert.equal(await page.$eval('#about-dialog', el => el.open), false);
  await page.click('#about-button');
  await page.waitForSelector('#about-dialog[open]');
  assert.equal(await page.$eval('#settings-menu', el => el.hidden), true);
  assert.deepEqual(await page.evaluate(() => [$('#about-version').textContent, $('#about-build').textContent]), await page.evaluate(() => [DEVSHELL.displayVersion || DEVSHELL.version, String(DEVSHELL.build)]));
  if (process.env.DENGSHELL_ABOUT_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_ABOUT_SCREENSHOT });
  await page.click('#close-about');
  assert.equal(await page.$eval('#about-dialog', el => el.open), false);
  assert.equal(await page.evaluate(() => document.activeElement.id), 'about-button');
  await page.click('#about-button');
  await page.keyboard.press('Escape');
  assert.equal(await page.$eval('#about-dialog', el => el.open), false);
  await page.click('#settings-button');
  await page.click('#manage-keys');
  await page.waitForSelector('#keys-dialog[open]');
  await page.click('#close-keys');
  checks.push('The gear next to the logo opens settings; the logo shows actual version/build, closes with X or Escape, and restores focus');
  assert.equal(await page.$('#about-software'), null);
  await page.evaluate(() => { window.qaOldOpen = window.open; window.qaOpened = []; window.open = (...args) => { qaOpened.push(args); return null; }; });
  try {
    await page.click('#github-button');
    assert.deepEqual(await page.evaluate(() => qaOpened), [['https://github.com/yuhongwei380/DevShell', '_blank', 'noopener,noreferrer']]);
  } finally { await page.evaluate(() => { window.open = qaOldOpen; delete window.qaOldOpen; }); }
  await page.evaluate(() => { window.qaOldGo = window.go; window.qaNativeRepository = 0; window.go = { desktop: { Desktop: { OpenAbout: async () => { qaNativeRepository++; } } } }; });
  try {
    await page.click('#github-button');
    assert.equal(await page.evaluate(() => qaNativeRepository), 1);
  } finally { await page.evaluate(() => { window.go = qaOldGo; delete window.qaOldGo; }); }
  await page.setOfflineMode(true);
  try {
    await page.click('#settings-button');
    await page.click('#release-notes-button');
    await page.waitForSelector('#release-notes-dialog[open]');
    assert.equal(await page.$eval('#settings-menu', el => el.hidden), true);
    assert.equal(await page.$eval('#release-notes-content h1', el => el.textContent), await page.evaluate(() => DEVSHELL.releaseNotes.split(/\r?\n/)[0].replace(/^#\s*/, '')));
    assert.equal(await page.$eval('#release-notes-version', el => el.textContent), await page.evaluate(() => `当前版本 ${DEVSHELL.displayVersion || DEVSHELL.version} · 构建 ${DEVSHELL.build}`));
    if (process.env.DENGSHELL_NOTES_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_NOTES_SCREENSHOT });
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => document.activeElement.id === 'settings-button');
    await page.click('#settings-button'); await page.click('#release-notes-button');
    await page.click('#close-release-notes');
    assert.equal(await page.$eval('#release-notes-dialog', el => el.open), false);
  } finally { await page.setOfflineMode(false); }
  checks.push('Settings shows bundled current-version release notes offline and removes About software; GitHub opens the project through browser and native handlers');
  const originalTheme = await page.evaluate(() => DevShellTheme.preference);
  const categoryParsing = await page.evaluate(() => {
    const container = document.createElement('div');
    container.innerHTML = '<p>新增：项目</p><p>修复: 项目</p><p>  1. 调整：项目</p><pre>新增：代码块</pre><p><code>修复：行内代码</code></p><p>正文中的调整：保持不变</p>';
    const original = container.textContent;
    DengReleaseNoteTags.highlight(container);
    const first = container.innerHTML;
    DengReleaseNoteTags.highlight(container);
    return { kinds: [...container.querySelectorAll('.release-note-tag')].map(tag => tag.dataset.kind), originalPreserved: original === container.textContent, repeatedUnchanged: first === container.innerHTML, codeTags: container.querySelectorAll('pre .release-note-tag,code .release-note-tag').length };
  });
  assert.deepEqual(categoryParsing, { kinds: ['added', 'fixed', 'changed'], originalPreserved: true, repeatedUnchanged: true, codeTags: 0 });
  async function assertCategoryColors(selector, expectedKinds = ['added', 'changed', 'fixed']) {
    const tags = await page.$eval(selector, container => {
      const luminance = css => {
        const channels = css.match(/[\d.]+/g).slice(0, 3).map(Number).map(value => { const c = value / 255; return c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4; });
        return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
      };
      return [...container.querySelectorAll('.release-note-tag')].map(tag => {
        const style = getComputedStyle(tag), fg = luminance(style.color), bg = luminance(style.backgroundColor);
        return { kind: tag.dataset.kind, weight: Number(style.fontWeight), contrast: (Math.max(fg, bg) + .05) / (Math.min(fg, bg) + .05), color: style.color };
      });
    });
    assert.deepEqual([...new Set(tags.map(tag => tag.kind))].sort(), expectedKinds);
    assert.equal(new Set(tags.map(tag => tag.color)).size, expectedKinds.length);
    assert(tags.every(tag => tag.weight >= 700 && tag.contrast >= 4.5), JSON.stringify(tags));
  }
  async function assertCategoryAlignment(selector) {
    const rows = await page.$eval(selector, container => [...container.querySelectorAll('.release-note-tag')].map(tag => {
      const bounds = tag.getBoundingClientRect();
      let text = tag.nextSibling;
      while (text && text.nodeType !== Node.TEXT_NODE) text = text.nextSibling;
      const range = document.createRange(); range.setStart(text, 0); range.setEnd(text, 1);
      return { left: bounds.left, width: bounds.width, body: range.getBoundingClientRect().left };
    }));
    assert(rows.length >= 3);
    for (const row of rows) {
      assert(Math.abs(row.left - rows[0].left) < .5 && Math.abs(row.width - rows[0].width) < .5 && Math.abs(row.body - rows[0].body) < .5, JSON.stringify(rows));
    }
  }
  const releaseKinds = await page.evaluate(() => {
    const kinds = { 新增: 'added', 修复: 'fixed', 调整: 'changed' };
    return [...new Set([...DEVSHELL.releaseNotes.matchAll(/(?:^|\n)\s*(?:[-*+]\s+)?(新增|修复|调整)[：:]/g)].map(match => kinds[match[1]]))].sort();
  });
  const categoryFixture = '新增：第一项\n修复: 第二项\n- 调整：第三项';
  for (const theme of ['light', 'dark']) {
    await page.evaluate(value => DevShellTheme.set(value), theme);
    await page.waitForFunction(() => !document.documentElement.dataset.lightSwitch);
    await page.click('#settings-button'); await page.click('#release-notes-button');
    await assertCategoryColors('#release-notes-content', releaseKinds);
    await page.$eval('#release-notes-content', (container, notes) => {
      container.innerHTML = window.markdownit({ html: false, breaks: true }).render(notes);
      DengReleaseNoteTags.highlight(container);
    }, categoryFixture);
    await assertCategoryAlignment('#release-notes-content');
    await page.click('#close-release-notes');
    await page.evaluate(notes => { window.qaUpdateDialog = DengUpdates.showUpdate({ currentVersion: DEVSHELL.version, currentBuild: DEVSHELL.build, latestVersion: DEVSHELL.version, latestBuild: DEVSHELL.build + 1, platform: 'windows-amd64', installerReady: false, notes }); }, categoryFixture);
    await page.waitForSelector('#startup-update-dialog[open]');
    await assertCategoryColors('#startup-update-notes');
    await assertCategoryAlignment('#startup-update-notes');
    assert.equal(await page.$eval('#startup-update-notes', el => el.textContent), categoryFixture);
    if (process.env.DENGSHELL_NOTE_TAGS_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_NOTE_TAGS_SCREENSHOT + '-' + theme + '.png' });
    await page.click('#startup-update-later');
    await page.evaluate(() => qaUpdateDialog);
  }
  await page.evaluate(value => DevShellTheme.set(value), originalTheme);
  await page.waitForFunction(() => !document.documentElement.dataset.lightSwitch);
  checks.push('Added/fixed/changed use distinct bold colors with 4.5:1 contrast; mixed list markers and colon widths keep labels and body starts aligned in both themes');
  assert.equal(await page.$eval('#add-session', el => el.title), '新选项卡');
  await assertPlusFollowsLastTab();
  await page.click('#add-session');
  await page.waitForSelector('.start-tab.active');
  await assertPlusFollowsLastTab();
  assert.equal(await page.$eval('#welcome-state', el => el.hidden), false);
  assert.equal(await page.$eval('#quick-connect-dialog', el => el.open), false);
  assert.equal(await page.evaluate(() => sessions.size), 0);
  await page.click('#welcome-quick-connect');
  await page.waitForSelector('#quick-connect-dialog[open]');
  await page.click('#quick-connect-dialog [data-close]');
  checks.push('The right-side + creates a frontend start page; quick connect remains available inside that page');
  await page.click('#add-session');
  await page.click('#add-session');
  assert.equal(await page.$eval('.start-tab.active .session-tab-label', el => el.textContent), '新选项卡 3');
  await page.click('.start-tab [aria-label="关闭 新选项卡 2"]');
  await page.click('.start-tab [aria-label="关闭 新选项卡 3"]');
  await page.click('#add-session');
  assert.equal(await page.$eval('.start-tab.active .session-tab-label', el => el.textContent), '新选项卡 2');
  // Closing a lower number must not reuse a number still open in another tab.
  await page.click('.start-tab [aria-label="关闭 新选项卡 1"]');
  await page.click('#add-session');
  assert.deepEqual(await page.$$eval('.start-tab .session-tab-label', els => els.map(el => el.textContent)), ['新选项卡 2', '新选项卡 3']);
  await page.click('.start-tab [aria-label="关闭 新选项卡 2"]');
  await page.click('.start-tab [aria-label="关闭 新选项卡 3"]');
  await page.click('#add-session');
  assert.equal(await page.$eval('.start-tab.active .session-tab-label', el => el.textContent), '新选项卡 1');
  checks.push('Closing tabs 2 and 3 lets the next start page reuse 2; live numbers never collide and closing all resets numbering to 1');
  // Create a real terminal view with a fake transport, never an SSH connection.
  await page.evaluate(() => {
    const profile = { id: 'tabs-live-profile', name: '保留的 SSH', host: 'example.invalid', user: 'qa', port: 22 };
    profiles.push(profile);
    const state = makeSessionState({ id: 'tabs-live', profileId: profile.id, home: '/', sftpAvailable: false });
    Object.assign(state, { localOnly: true, connected: false, tabOrder: nextSessionOrder++ });
    sessions.set(state.id, state); createTerminal(state);
    state.connected = true; state.localOnly = false;
    window.qaSocketCloses = 0; state.ws = { readyState: WebSocket.OPEN, send() {}, close() { qaSocketCloses++; } };
    activate(state.id);
  });
  await assertPlusFollowsLastTab();
  checks.push('The + follows server management when empty and the last tab as start pages or connected sessions are added');
  await page.click('#add-session'); await settle();
  assert.equal(await page.evaluate(() => sessions.get('tabs-live').connected && sessions.get('tabs-live').host.hidden), true);
  assert.equal(await page.evaluate(() => qaSocketCloses), 0);
  await page.click('[data-session-id="tabs-live"] [role="tab"]'); await settle();
  assert.equal(await page.evaluate(() => activeID), 'tabs-live');
  assert.equal(await page.$eval('#welcome-state', el => el.hidden), true);
  checks.push('Start pages hide the current terminal while preserving its connection; selecting its tab restores it');
  await page.evaluate(() => { for (let i = 0; i < 40; i++) DengStartTabs.create(); });
  await settle();
  assert.equal(await page.$$eval('.session-tab', els => new Set(els.map(el => el.offsetTop)).size), 1);
  await page.$eval('#session-tabs', el => { el.scrollLeft = 0; }); await settle();
  assert.equal(await page.$eval('#session-tabs-previous', el => el.disabled), true);
  assert.equal(await page.$eval('#session-tabs-next', el => el.disabled), false);
  await page.click('#session-tabs-next'); await settle();
  assert((await page.$eval('#session-tabs', el => el.scrollLeft)) > 0);
  await page.click('#session-tabs-previous'); await settle();
  assert((await page.$eval('#session-tabs', el => el.scrollLeft)) <= 1);
  await page.click('#session-tabs-list');
  await page.waitForSelector('#session-tabs-menu:not([hidden])');
  assert.equal(await page.$$eval('#session-tabs-menu button', els => els.length), 43);
  assert.equal(await page.$$eval('#session-tabs-menu [aria-current="page"]', els => els.length), 1);
  await page.click('#session-tabs-menu button'); await settle();
  assert.equal(await page.evaluate(() => DengStartTabs.has(activeID)), true);
  assert.equal(await page.$eval('#session-tabs-menu', el => el.hidden), true);
  checks.push('43 tabs stay on one row; arrows move the viewport and the dropdown lists every tab with the active tab checked');
  // The list must use each tab's real action for SSH, RDP and process views.
  const kinds = await page.evaluate(() => {
    const calls = [];
    for (const [kind, label] of [['rdp', '测试 RDP'], ['process', '进程 · 测试']]) {
      const tab = node('div', `session-tab ${kind}-tab`), button = node('button');
      button.setAttribute('role', 'tab'); button.append(node('span', 'session-tab-label', label));
      button.onclick = () => calls.push(kind); tab.append(button); $('#session-tabs').append(tab);
    }
    window.qaKindCalls = calls;
    return calls.length;
  });
  assert.equal(kinds, 0); await settle();
  await page.click('#session-tabs-list');
  await page.evaluate(() => [...$('#session-tabs-menu').children].find(el => el.textContent.includes('测试 RDP')).click());
  await page.click('#session-tabs-list');
  await page.evaluate(() => [...$('#session-tabs-menu').children].find(el => el.textContent.includes('进程 · 测试')).click());
  assert.deepEqual(await page.evaluate(() => qaKindCalls), ['rdp', 'process']);
  await page.evaluate(() => { document.querySelectorAll('#session-tabs > .rdp-tab, #session-tabs > .process-tab').forEach(el => el.remove()); });
  checks.push('The dropdown dispatches RDP and process tab actions without treating them as SSH tabs');
  for (const [width, height, scale] of [[1440,1000,1], [1024,768,1], [800,600,1], [640,480,1], [480,360,1], [1440,1000,1.5], [1440,1000,2]]) {
    await page.setViewport({ width, height });
    await page.evaluate(value => { appearance.uiScale = value; applyUIScale(); }, scale); await settle();
    const metrics = await page.evaluate(() => {
      const plus = $('#add-session').getBoundingClientRect(), tabs = $('#session-tabs').getBoundingClientRect();
      const hit = document.elementFromPoint(plus.x + plus.width / 2, plus.y + plus.height / 2);
      const brandVisible = ['about-button', 'settings-button', 'github-button'].every(id => { const r = document.getElementById(id).getBoundingClientRect(); return document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.closest('#' + id); });
      return { left: plus.left, right: plus.right, tabsRight: tabs.right, visible: !!hit?.closest('#add-session'), brandVisible, rows: new Set([...$('#session-tabs').children].map(el => el.offsetTop)).size };
    });
    assert(metrics.visible && metrics.brandVisible && metrics.right <= width + 1 && metrics.left >= metrics.tabsRight - 1 && metrics.rows === 1, JSON.stringify({width, scale, metrics}));
    await page.click('#session-tabs-list');
    assert.equal(await page.$eval('#session-tabs-menu', el => { const r = el.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1; }), true);
    await page.keyboard.press('Escape');
  }
  checks.push('The + remains clickable on the right at 480–1440px and 100–200% zoom; dropdown stays inside the viewport');
  await page.setViewport({ width: 1440, height: 1000 });
  await page.evaluate(() => { appearance.uiScale = 1; applyUIScale(); }); await settle();
  // Reordering a start page before a live tab must survive conversion to SSH.
  const replacement = await page.evaluate(async () => {
    const page = DengStartTabs.create();
    const tab = [...$('#session-tabs').children].find(el => el.dataset.startTabId === page.id);
    DengTabOrder.commit(tab, { before: $('#session-tabs').firstElementChild.dataset.tabKey });
    const oldApi = api, oldCreate = createTerminal, oldRefresh = DengCommandHistory.refresh;
    const profile = { id: 'tabs-replace-profile', name: '起始页建立的连接', host: 'example.invalid', user: 'qa', port: 22, auth: 'agent' };
    profiles.push(profile);
    let opened = 0;
    api = async (url, options) => url === '/api/sessions' ? { id: ++opened === 1 ? 'tabs-replaced' : 'tabs-background', profileId: profile.id, home: '/', sftpAvailable: false } : oldApi(url, options);
    createTerminal = state => { oldCreate(state); state.openTerminalSocket = () => {}; };
    DengCommandHistory.refresh = async () => {};
    try {
      const state = await connect(profile.id, false, { refreshHistory: false });
      const backgroundPage = DengStartTabs.create();
      await connect(profile.id, false, { background: true, refreshHistory: false });
      return { connected: !!state?.connected, removed: !DengStartTabs.has(page.id), first: $('#session-tabs').firstElementChild.dataset.sessionId, preserved: sessions.get('tabs-live').connected, socketCloses: qaSocketCloses, background: activeID === backgroundPage.id && DengStartTabs.has(backgroundPage.id) };
    } finally { api = oldApi; createTerminal = oldCreate; DengCommandHistory.refresh = oldRefresh; }
  });
  assert.deepEqual(replacement, { connected: true, removed: true, first: 'tabs-replaced', preserved: true, socketCloses: 0, background: true });
  checks.push('Foreground connects replace a reordered start page in place; background connects preserve that page and selection');
  await page.click('#add-session'); await settle();
  await page.click('.start-tab.active .tab-close'); await settle();
  assert.equal(await page.evaluate(() => qaSocketCloses), 0);
  assert.equal(await page.$$eval('.session-tab.active', els => els.length), 1);
  checks.push('Closing a start page activates a neighbor without disconnecting another session');
  if (process.env.DENGSHELL_TABS_SCREENSHOT) {
    await page.evaluate(() => { DevShellTheme.set('dark'); DengStartTabs.create(); });
    await page.waitForFunction(() => !document.documentElement.dataset.lightSwitch); await settle();
    await page.click('#session-tabs-list');
    await page.screenshot({ path: process.env.DENGSHELL_TABS_SCREENSHOT });
  }
  assert.deepEqual(errors, []);
  console.log('PASS', checks);
} catch (error) {
  if (page && process.env.DENGSHELL_TABS_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_TABS_SCREENSHOT });
  console.error('Page errors:', errors); throw error;
} finally {
  try { await browser?.close(); } catch {}
  try { child.kill(); } catch {}
  try { fs.rmSync(profileDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 150 }); } catch {}
}
