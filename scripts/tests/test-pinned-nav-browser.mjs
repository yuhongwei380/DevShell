// Opt-in browser regression for the pinned links dropdown: the session bar
// carries an expand button right of the server manager (the header row is
// gone, the shell reaches the top), and the button drops a vertical list of
// the pinned connections. Unpinning goes through the shared confirmation
// dialog; right-click renames in place; a real drag reorders vertically and
// persists; choosing a connection closes the list like any menu.
// Edge on Windows hands the browser process off from the launcher stub, so the
// browser is spawned directly and driven over CDP instead of puppeteer.launch.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
const [stage, chrome, modulePath] = process.argv.slice(2);
const { default: puppeteer } = await import(path.isAbsolute(modulePath) ? pathToFileURL(modulePath).href : modulePath);
const fixture = JSON.parse(fs.readFileSync(stage + '/browser-fixture.json'));
const endpoint = new URL(fixture.url).origin;
async function api(pathname, body) { const r = await fetch(endpoint + pathname, { method: body === undefined ? 'GET' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': fixture.token }, body: body === undefined ? undefined : JSON.stringify(body) }); const data = await r.json(); return { status: r.status, data }; }
await api('/api/appearance/patch', { onboardingCompleted: true, uiScale: 1 });
const created = [];
for (let i = 0; i < 8; i++) { const { status, data } = await api('/api/profiles', { name: `固定测试-${i + 1}`, host: '192.0.2.77', port: 22, user: 'root' }); assert.equal(status, 200, `create profile ${i + 1}`); created.push(data.id); }
await api('/api/appearance/patch', { layout: { 'dengshell.pinned.v1': created } });
const profileDir = fs.mkdtempSync(path.join(path.resolve(stage), 'edge-profile-'));
const child = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--noerrdialogs', `--user-data-dir=${profileDir}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore', windowsHide: true });
let browser;
const checks = [], errors = []; let page;
try {
  let wsEndpoint = null;
  for (let attempt = 0; attempt < 100 && !wsEndpoint; attempt++) {
    const portFile = path.join(profileDir, 'DevToolsActivePort');
    if (fs.existsSync(portFile)) {
      const [port, wsPath] = fs.readFileSync(portFile, 'utf8').trim().split(/\r?\n/);
      if (port && wsPath) wsEndpoint = `ws://127.0.0.1:${port.trim()}${wsPath.trim()}`;
    }
    if (!wsEndpoint) await new Promise(resolve => setTimeout(resolve, 200));
  }
  if (!wsEndpoint) throw new Error('headless Edge did not expose DevToolsActivePort');
  browser = await puppeteer.connect({ browserWSEndpoint: wsEndpoint });
  page = await browser.newPage(); page.on('pageerror', e => errors.push(e.message)); await page.setViewport({ width: 2800, height: 900 });
  await page.goto(fixture.url, { waitUntil: 'networkidle0' });
  await page.waitForFunction(() => window.DengPinnedNav && !document.querySelector('#pinned-toggle').hidden);
  // There is no titlebar row any more: the app buttons live in the session
  // row, the shell reaches the top of the window, and the pinned links float
  // in a dropdown that never shifts the layout.
  const collapsedState = await page.evaluate(() => {
    const t = document.querySelector('#pinned-toggle').getBoundingClientRect();
    const c = document.querySelector('#connection-button').getBoundingClientRect();
    const row = document.querySelector('.session-row').getBoundingClientRect();
    const ws = document.querySelector('#workspace').getBoundingClientRect();
    return {
      besideManager: t.left >= c.right - 1 && t.top < c.bottom && t.bottom > c.top,
      monitorAligned: (() => {
        const s = document.querySelector('.sidebar .system-section').getBoundingClientRect();
        const w = document.querySelector('#workspace').getBoundingClientRect();
        return Math.abs(s.top - w.top) < 2;
      })(),
      brandAligned: (() => {
        const b = document.querySelector('.session-bar .brand').getBoundingClientRect();
        return Math.abs((b.top + b.bottom) / 2 - (c.top + c.bottom) / 2) < 3;
      })(),
      rowTopLogical: row.top / effectiveScale,
      barHidden: document.querySelector('#pinned-bar').hidden,
      workspaceTop: ws.top, workspaceHeight: ws.height,
    };
  });
  assert(collapsedState.besideManager, 'the expand button sits right of the server manager button');
  assert(collapsedState.brandAligned, 'the app buttons share the session row center line');
  assert(collapsedState.monitorAligned, 'the monitor column card top aligns with the shell top');
  assert(collapsedState.rowTopLogical < 14, 'the session row (and with it the shell) reaches the top of the window');
  assert.equal(collapsedState.barHidden, true, 'the links dropdown starts collapsed');
  checks.push('No titlebar row: the shell reaches the top and the links live behind an expand button next to the server manager');
  if (process.env.DENGSHELL_PINNED_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_PINNED_SCREENSHOT.replace(/\.png$/, '-collapsed.png') });
  await page.click('#pinned-toggle');
  await page.waitForFunction(() => !document.querySelector('#pinned-bar').hidden && document.querySelectorAll('.pinned-item').length === 8);
  const openState = await page.evaluate(() => ({
    position: getComputedStyle(document.querySelector('#pinned-bar')).position,
    width: document.querySelector('#pinned-bar').getBoundingClientRect().width,
    top: document.querySelector('#workspace').getBoundingClientRect().top,
    height: document.querySelector('#workspace').getBoundingClientRect().height,
    anchored: (() => {
      const t = document.querySelector('#pinned-toggle').getBoundingClientRect();
      const b = document.querySelector('#pinned-bar').getBoundingClientRect();
      const items = [...document.querySelectorAll('.pinned-item')];
      const vertical = items.every((el, i) => i === 0 || (el.getBoundingClientRect().top > items[i - 1].getBoundingClientRect().bottom - 1 && Math.abs(el.getBoundingClientRect().left - items[0].getBoundingClientRect().left) < 1));
      return b.top > t.bottom && b.left >= 0 && b.right <= innerWidth + 1 && b.bottom <= innerHeight + 1 && vertical;
    })(),
  }));
  assert.equal(openState.position, 'fixed', 'the links dropdown floats instead of reserving layout');
  assert(Math.abs(openState.top - collapsedState.workspaceTop) < 1 && Math.abs(openState.height - collapsedState.workspaceHeight) < 1, 'opening the dropdown keeps the shell at full size');
  assert(openState.anchored, 'the dropdown anchors below the button as one vertical column inside the viewport');
  await page.click('#pinned-toggle');
  assert.equal(await page.$eval('#pinned-bar', el => el.hidden), true, 'a second click collapses the dropdown');
  await page.click('#pinned-toggle');
  await page.waitForFunction(() => !document.querySelector('#pinned-bar').hidden);
  checks.push('The toggle drops a vertical list anchored under it; the shell never resizes');
  if (process.env.DENGSHELL_PINNED_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_PINNED_SCREENSHOT.replace(/\.png$/, '-open.png') });
  // Window buttons moved into the same session row; frameless mode shows them
  // at the right end of that row.
  await page.evaluate(() => reflectWindowState({ frameless: true }));
  const controls = await page.evaluate(() => {
    const wc = document.querySelector('#window-controls'), row = document.querySelector('.session-row');
    const r = wc.getBoundingClientRect(), rr = row.getBoundingClientRect();
    return { hidden: wc.hidden, sameRow: r.top >= rr.top - 1 && r.bottom <= rr.bottom + 1, right: r.right };
  });
  assert.equal(controls.hidden, false, 'frameless mode shows the window buttons');
  assert(controls.sameRow && controls.right <= 2800 + 1, 'the window buttons live at the right end of the session row');
  checks.push('Frameless window buttons share the session row at its right end');
  const inspectDropdown = async () => page.evaluate(() => {
    const bar = document.querySelector('#pinned-bar'), rect = bar.getBoundingClientRect();
    const items = [...bar.querySelectorAll('.pinned-item')];
    return {
      visible: items.length,
      inside: rect.left >= 0 && rect.right <= innerWidth + 1 && rect.bottom <= innerHeight + 1,
      clipped: items.some(el => { const r = el.getBoundingClientRect(); return r.left < rect.left - 1 || r.right > rect.right + 1 || r.top < rect.top - 1 || r.bottom > Math.max(rect.bottom, rect.top + rect.height) + 1; }),
      labels: items.map(el => el.querySelector('.pinned-item-open span').textContent),
    };
  });
  const expandPanel = async () => {
    await page.waitForFunction(() => !document.querySelector('#pinned-toggle').hidden);
    await page.evaluate(() => { if (document.querySelector('#pinned-bar').hidden) document.querySelector('#pinned-toggle').click(); });
    await page.waitForFunction(() => !document.querySelector('#pinned-bar').hidden);
  };
  // A real resize must keep the whole list reachable inside the viewport; the
  // list scrolls, so no entry is ever cropped or hidden behind a menu.
  for (const width of [1440, 1100, 800, 700, 520]) {
    await page.setViewport({ width, height: 900 });
    // headless setViewport does not always deliver a window resize; dispatch
    // it so the app refreshes --view-width and the dropdown re-anchors.
    await page.evaluate(() => window.dispatchEvent(new Event('resize')));
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await expandPanel();
    const layout = await inspectDropdown();
    assert.equal(layout.inside, true, `the dropdown stays inside the viewport at width ${width}`);
    assert.equal(layout.clipped, false, `no clipped connection at width ${width}`);
    assert.equal(layout.visible, 8, `all pins listed at width ${width}`);
  }
  checks.push('Resizing at 1440/1100/800/700/520px keeps every pin listed and inside the viewport');
  await page.setViewport({ width: 1000, height: 900 });
  await page.evaluate(() => { appearance.uiScale = 1.5; applyUIScale(); });
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expandPanel();
  assert.equal((await inspectDropdown()).inside, true, '150% application zoom keeps the dropdown inside the viewport');
  await page.evaluate(() => { appearance.uiScale = 1; applyUIScale(); });
  checks.push('150% application zoom keeps the dropdown anchored inside the viewport');
  // Use real pointer input: crossing the drag threshold must not also open a
  // session. Vertical reordering persists through the layout API.
  const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const item = id => `.pinned-item[data-pinned-id="${id}"]`;
  const startDrag = async selector => {
    const box = await (await page.$(`${selector} .pinned-item-open`)).boundingBox();
    assert(box, `visible drag source ${selector}`);
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 + 9, { steps: 3 });
  };
  const moveTo = async (selector, after = false) => {
    const box = await (await page.$(selector)).boundingBox();
    assert(box, `visible drop target ${selector}`);
    await page.mouse.move(box.x + box.width * .5, box.y + box.height * (after ? .8 : .2), { steps: 12 });
  };
  const assertOrder = async expected => {
    await settle();
    assert.deepEqual(await page.evaluate(() => appearance.layout['dengshell.pinned.v1']), expected);
    assert.deepEqual(await page.$$eval('.pinned-item', els => els.map(el => el.dataset.pinnedId)), expected);
    assert.deepEqual((await api('/api/config')).data.appearance.layout['dengshell.pinned.v1'], expected);
    assert.equal(await page.$('.pinned-sorting, .pinned-drag-source, .pinned-drop-before, .pinned-drop-after'), null);
  };
  const drop = async expected => {
    const saved = page.waitForResponse(r => r.url().endsWith('/api/appearance/patch') && r.request().method() === 'POST');
    await page.mouse.up();
    assert.equal((await saved).status(), 200);
    await assertOrder(expected);
  };
  await page.setViewport({ width: 2800, height: 900 }); await page.evaluate(() => window.dispatchEvent(new Event('resize'))); await settle();
  await expandPanel();
  await page.evaluate(() => { window.qaSortConnect = connect; window.qaSortCalls = []; connect = id => window.qaSortCalls.push(id); });
  await startDrag(item(created[0]));
  await moveTo(item(created[2]), true);
  assert.equal(await page.$eval(item(created[2]), el => el.classList.contains('pinned-drop-after')), true);
  await drop([created[1], created[2], created[0], ...created.slice(3)]);
  await startDrag(item(created[0])); await moveTo(item(created[1])); await drop(created);
  // Escape and out-of-list drops leave both the DOM and the saved layout alone.
  await startDrag(item(created[0])); await moveTo(item(created[2]));
  await page.keyboard.press('Escape'); await page.mouse.up(); await assertOrder(created);
  await startDrag(item(created[0])); await page.mouse.move(30, 700); await page.mouse.up(); await assertOrder(created);
  assert.deepEqual(await page.evaluate(() => window.qaSortCalls), [], 'a drag must never connect');
  // Choosing an entry connects and closes the dropdown, like any menu.
  await page.click(`${item(created[0])} .pinned-item-open`);
  assert.deepEqual(await page.evaluate(() => window.qaSortCalls), [created[0]], 'an ordinary click still connects after dragging');
  await settle();
  assert.equal(await page.$eval('#pinned-bar', el => el.hidden), true, 'choosing a connection closes the dropdown');
  checks.push('Vertical drags reorder in both directions; Escape/outside drops cancel; choosing an entry connects and closes the list');
  // A failed write rolls back exactly this layout key and surfaces the error.
  await expandPanel();
  await page.setRequestInterception(true);
  const rejectSave = request => {
    if (request.url().endsWith('/api/appearance/patch') && request.method() === 'POST') {
      request.respond({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: '排序保存失败测试' }) });
    } else request.continue();
  };
  page.on('request', rejectSave);
  await startDrag(item(created[0])); await moveTo(item(created[2]), true); await page.mouse.up();
  await page.waitForFunction(() => document.querySelector('#toast')?.textContent.includes('排序保存失败测试'));
  await assertOrder(created);
  await page.setRequestInterception(false); page.off('request', rejectSave);
  // Drop coordinates must be correct at application zoom too.
  await page.evaluate(() => { appearance.uiScale = 1.5; applyUIScale(); }); await settle();
  await expandPanel();
  await startDrag(item(created[1])); await moveTo(item(created[0]));
  const reordered = [created[1], created[0], ...created.slice(2)]; await drop(reordered);
  await page.evaluate(() => { connect = window.qaSortConnect; delete window.qaSortConnect; delete window.qaSortCalls; });
  await page.reload({ waitUntil: 'networkidle0' });
  await page.waitForFunction(() => window.DengPinnedNav && !document.querySelector('#pinned-toggle').hidden);
  assert.equal(await page.$eval('#pinned-bar', el => el.hidden), true, 'a reload collapses the dropdown again');
  await page.click('#pinned-toggle');
  await page.waitForFunction(() => !document.querySelector('#pinned-bar').hidden && document.querySelectorAll('.pinned-item').length === 8);
  await assertOrder(reordered);
  checks.push('Failed saves roll back; 150% zoom drops persist and survive a reload that starts collapsed');
  // Delete asks for confirmation; cancelling keeps the pin and the layout. The
  // dialog is not an outside click: the dropdown stays open throughout.
  let items = await page.$$('.pinned-item');
  await (await items[0].$('.pinned-item-remove')).click();
  await page.waitForSelector('#action-dialog[open]');
  assert((await page.$eval('#action-title', e => e.textContent)).includes('取消固定'));
  await page.click('#action-cancel');
  await page.waitForFunction(() => !document.querySelector('#action-dialog').open);
  assert.equal(await page.$eval('#pinned-bar', el => el.hidden), false, 'the dropdown stays open through the confirmation dialog');
  assert.equal(await page.$$eval('.pinned-item', els => els.length), 8);
  assert.deepEqual(await page.evaluate(() => appearance.layout['dengshell.pinned.v1']), reordered);
  checks.push('Delete asks for confirmation, the dropdown stays open through dialogs and cancel changes nothing');
  // Confirming removes exactly that pin and persists the layout.
  items = await page.$$('.pinned-item');
  const removedId = await items[0].evaluate(el => el.dataset.pinnedId);
  await (await items[0].$('.pinned-item-remove')).click();
  await page.waitForSelector('#action-dialog[open]');
  await page.click('#action-confirm');
  await page.waitForFunction(() => appearance.layout['dengshell.pinned.v1'].length === 7 && document.querySelectorAll('.pinned-item').length === 7);
  assert.deepEqual(await page.evaluate(() => appearance.layout['dengshell.pinned.v1']), reordered.filter(id => id !== removedId));
  checks.push('Confirming removes the pin and persists the layout');
  // Right-click opens the rename dialog prefilled with the current name;
  // cancelling changes nothing anywhere.
  const renameTarget = await page.$eval('.pinned-item', el => el.dataset.pinnedId);
  const firstName = await page.$eval('.pinned-item .pinned-item-open span', el => el.textContent);
  await page.evaluate(() => document.querySelector('.pinned-item').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true })));
  await page.waitForSelector('#action-dialog[open]');
  assert((await page.$eval('#action-title', e => e.textContent)).includes('重命名连接'));
  assert.equal(await page.$eval('#action-input', e => e.value), firstName);
  await page.click('#action-cancel');
  await page.waitForFunction(() => !document.querySelector('#action-dialog').open);
  assert.equal(await page.$eval('.pinned-item .pinned-item-open span', el => el.textContent), firstName);
  // Confirming renames the saved profile: the list row, the in-app profile list
  // (the same name the server manager edits) and the persisted config update.
  const renamed = '改名后的连接';
  await page.evaluate(() => document.querySelector('.pinned-item').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true })));
  await page.waitForSelector('#action-dialog[open]');
  await page.$eval('#action-input', (el, value) => { el.value = value; }, renamed);
  await page.click('#action-confirm');
  await page.waitForFunction(name => document.querySelector('.pinned-item .pinned-item-open span')?.textContent === name, {}, renamed);
  assert.equal(await page.evaluate(id => profiles.find(profile => profile.id === id)?.name, renameTarget), renamed);
  assert.equal((await api('/api/config')).data.servers.find(profile => profile.id === renameTarget).name, renamed);
  assert.deepEqual(await page.evaluate(() => appearance.layout['dengshell.pinned.v1']), reordered.filter(id => id !== removedId));
  checks.push('Right-click renames the shared profile name and the persisted config; pins stay put');
  // Clicking a row always opens a fresh session, even when one for that
  // profile is already open and active.
  const clickCalls = await page.evaluate(() => {
    const calls = { connect: [], activate: [] };
    const originalConnect = connect, originalActivate = activate;
    connect = id => { calls.connect.push(id); return Promise.resolve(null); };
    activate = id => { calls.activate.push(id); };
    const target = appearance.layout['dengshell.pinned.v1'][0];
    sessions.set('qa-existing', { id: 'qa-existing', profileId: target, connected: true });
    activeID = 'qa-existing';
    document.querySelector('.pinned-item .pinned-item-open').click();
    sessions.delete('qa-existing'); activeID = null;
    connect = originalConnect; activate = originalActivate;
    return calls;
  });
  assert.deepEqual(clickCalls.connect, [reordered[1]], 'list click must call connect for the pinned profile');
  assert.deepEqual(clickCalls.activate, [], 'list click must not merely activate the existing session');
  checks.push('Clicking a row with an existing active session still opens a new connection');
  // Long names must ellipsize inside the fixed-width list instead of cropping,
  // regardless of monitor visibility or sidebar width.
  await expandPanel();
  await page.evaluate(() => {
    profiles.find(profile => profile.id === appearance.layout['dengshell.pinned.v1'][1]).name = '很长的服务器连接名称 '.repeat(5);
    DengPinnedNav.render();
    appearance.monitorVisible = false; applyWorkspacePlacement();
  });
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expandPanel();
  let longState = await page.evaluate(() => {
    const bar = document.querySelector('#pinned-bar'), rect = bar.getBoundingClientRect();
    const spans = [...bar.querySelectorAll('.pinned-item-open span')];
    return {
      clipped: [...bar.querySelectorAll('.pinned-item')].some(el => { const r = el.getBoundingClientRect(); return r.left < rect.left - 1 || r.right > rect.right + 1; }),
      ellipsis: spans[1] && spans[1].scrollWidth > spans[1].clientWidth,
    };
  });
  assert.equal(longState.clipped, false, 'long names never overflow the list');
  assert(longState.ellipsis, 'long names ellipsize inside the row');
  await page.evaluate(() => { appearance.monitorVisible = true; applyWorkspacePlacement(); document.documentElement.style.setProperty('--sidebar-width', '540px'); });
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expandPanel();
  assert.equal((await inspectDropdown()).clipped, false, 'monitor and sidebar changes keep the list intact');
  checks.push('Long names ellipsize; monitor visibility and sidebar width changes never crop the list');
  // The button stays available even with zero pins; an open dropdown keeps
  // showing an empty-state hint, and toggling reopens it anytime.
  await page.evaluate(async () => { appearance.layout['dengshell.pinned.v1'] = []; await persistAppearance({ layout: { 'dengshell.pinned.v1': [] } }); DengPinnedNav.render(); });
  await settle();
  assert.equal(await page.$eval('#pinned-toggle', el => el.hidden), false, 'the expand button stays visible with zero pins');
  assert.equal(await page.$eval('#pinned-bar', el => el.hidden), false, 'an open dropdown stays open with the empty-state hint');
  assert.equal(await page.$$eval('.pinned-item', els => els.length), 0);
  assert((await page.$eval('.pinned-empty', el => el.textContent)).includes('还没有固定的连接'), 'the empty dropdown explains how to pin');
  await page.click('#pinned-toggle');
  assert.equal(await page.$eval('#pinned-bar', el => el.hidden), true, 'the toggle collapses the empty dropdown');
  await page.click('#pinned-toggle');
  await page.waitForFunction(() => !document.querySelector('#pinned-bar').hidden);
  assert.equal(await page.$$eval('.pinned-empty', els => els.length), 1, 'the empty dropdown reopens');
  checks.push('The expand button is always visible; zero pins shows the empty-state hint');
  if (process.env.DENGSHELL_PINNED_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_PINNED_SCREENSHOT });
  assert.deepEqual(errors, []);
  fs.writeFileSync(stage + '/pinned-nav.json', JSON.stringify({ passed: true, checks, errors }, null, 2)); console.log('PASS', checks);
} catch (error) {
  console.error('pageerrors:', errors);
  if (page) {
    await page.screenshot({ path: stage + '/pinned-nav-failure.png' });
    console.error('Browser state:', await page.evaluate(() => {
      const r = document.querySelector('.brand').getBoundingClientRect();
      return { dialogs: [...document.querySelectorAll('dialog[open]')].map(el => el.id), brand: r.toJSON(), target: document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.outerHTML.slice(0, 300) };
    }));
  }
  throw error;
} finally { try { await browser?.close(); } catch {} try { child.kill(); } catch {} try { fs.rmSync(profileDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 }); } catch {} }
