// Isolated integration regression: no real connections or user data.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
const [stage, chrome, modulePath] = process.argv.slice(2);
const { default: puppeteer } = await import(path.isAbsolute(modulePath) ? pathToFileURL(modulePath).href : modulePath);
const fixture = JSON.parse(fs.readFileSync(path.join(stage, 'browser-fixture.json')));
const api = async (pathname, body) => {
  const response = await fetch(new URL(pathname, fixture.url), { method: body === undefined ? 'GET' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': fixture.token }, body: body === undefined ? undefined : JSON.stringify(body) });
  const data = await response.json(); assert(response.ok, JSON.stringify(data)); return data;
};
await api('/api/appearance/patch', { onboardingCompleted: true, uiScale: 1, layout: { 'cloudshell.layout': { conciseMode: false } } });
const source = await api('/api/group-nodes', { name: '来源' });
const parent = await api('/api/group-nodes', { name: '生产' });
const target = await api('/api/group-nodes', { name: '同名分组', parentId: parent.id });
const other = await api('/api/group-nodes', { name: '同名分组' });
const create = (name, groupId) => api('/api/profiles', { name, groupId, user: 'root', host: '192.0.2.1', port: 22, auth: 'password', secret: 'test-secret' });
const moved = await create('移动测试', source.id), kept = await create('留在原组', source.id), nested = await create('子组连接', target.id);
const profileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'devshell-move-browser-'));
const child = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--noerrdialogs', `--user-data-dir=${profileDir}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore', windowsHide: true });
let browser;
const errors = [];
try {
  let wsEndpoint;
  for (let attempt = 0; attempt < 100 && !wsEndpoint; attempt++) {
    const portFile = path.join(profileDir, 'DevToolsActivePort');
    if (fs.existsSync(portFile)) { const [port, wsPath] = fs.readFileSync(portFile, 'utf8').trim().split(/\r?\n/); if (port && wsPath) wsEndpoint = `ws://127.0.0.1:${port.trim()}${wsPath.trim()}`; }
    if (!wsEndpoint) await new Promise(resolve => setTimeout(resolve, 200));
  }
  assert(wsEndpoint, 'headless browser is available');
  browser = await puppeteer.connect({ browserWSEndpoint: wsEndpoint });
  const page = await browser.newPage(); page.on('pageerror', error => errors.push(error.message));
  await page.setViewport({ width: 1440, height: 1000 });
  await page.goto(fixture.url, { waitUntil: 'networkidle0' });
  await page.evaluate(async () => { await loadProfiles(); setDrawer(true); });
  const folder = id => `#server-folder-list [data-group-id="${id}"] .server-folder-select`;
  const card = id => `#connection-groups [data-profile-id="${id}"] .connection-card`;
  const listed = () => page.$$eval('#connection-groups [data-profile-id]', rows => rows.map(row => row.dataset.profileId).sort());
  await page.click(folder(source.id)); assert.deepEqual(await listed(), [kept.id, moved.id].sort());
  await page.click(folder(parent.id)); assert.deepEqual(await listed(), [nested.id]);
  await page.click('#server-include-children'); assert.deepEqual(await listed(), []);
  await page.click(folder(target.id)); assert.deepEqual(await listed(), [nested.id]);
  await page.click(folder(other.id)); assert.deepEqual(await listed(), []);
  await page.click(folder(source.id));
  const openMove = async () => {
    await page.click(card(moved.id), { button: 'right' });
    const action = await page.waitForSelector('#server-group-menu button');
    await action.dispose();
    await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '移动到…').click());
    await page.waitForSelector('#server-move-dialog[open]');
  };
  await openMove();
  const options = await page.$$eval('#server-move-group option', options => options.map(option => ({ id: option.value, text: option.textContent })));
  assert.equal(options.length, (await api('/api/config')).groupNodes.length);
  assert(options.find(option => option.id === target.id).text.includes('生产'), 'duplicate names show ancestor path');
  assert.equal(await page.$eval('#server-move-group', element => element.value), source.id);
  await page.keyboard.press('Escape'); assert.equal(await page.$('#server-move-dialog[open]'), null);
  assert.equal((await api('/api/config')).servers.find(profile => profile.id === moved.id).groupId, source.id);
  await openMove();
  await page.click('#server-move-dialog .themed-select-trigger');
  await page.click(`#server-move-dialog [role="option"][data-index="${options.findIndex(option => option.id === target.id)}"]`);
  assert.equal(await page.$eval('#server-move-group', element => element.value), target.id);
  await page.click('#save-server-move');
  await page.waitForFunction(id => !document.querySelector('#server-move-dialog').open && serverManager.selectedGroup === id, {}, target.id);
  assert.deepEqual(await listed(), [moved.id, nested.id].sort());
  assert.equal(await page.$eval(card(moved.id), button => button.getAttribute('aria-pressed')), 'true');
  const count = id => page.$eval(`#server-folder-list [data-group-id="${id}"] .server-folder-count`, element => element.textContent);
  assert.equal(await count(source.id), '1'); assert.equal(await count(parent.id), '2'); assert.equal(await count(target.id), '2');
  await page.click(folder(source.id)); assert.deepEqual(await listed(), [kept.id]);
  await page.click(folder(other.id)); assert.deepEqual(await listed(), []);
  await page.click(folder(target.id));
  // A stale destination leaves the dialog open and the saved connection intact.
  await openMove(); await page.select('#server-move-group', other.id);
  await page.setRequestInterception(true);
  page.on('request', request => request.url().endsWith(`/api/profiles/${moved.id}/group`) ? request.respond({ status: 400, contentType: 'application/json', body: JSON.stringify({ error: '目标分组不存在' }) }) : request.continue());
  await page.click('#save-server-move');
  await page.waitForSelector('#server-move-error:not([hidden])');
  assert.equal(await page.$eval('#save-server-move', button => button.disabled), false);
  assert.equal((await api('/api/config')).servers.find(profile => profile.id === moved.id).groupId, target.id);
  await page.click('#cancel-server-move');
  await page.reload({ waitUntil: 'networkidle0' });
  await page.evaluate(async () => { await loadProfiles(); setDrawer(true); });
  await page.click(folder(parent.id)); await page.click(folder(target.id));
  assert.deepEqual(await listed(), [moved.id, nested.id].sort());
  // The profile menu also clones a connection under the same name — the stored
  // credential travels with the clone inside the store — and pins connections
  // straight into the links bar.
  const openMenu = async (id, folderId = target.id) => {
    await page.click(folder(folderId));
    await page.waitForSelector(card(id));
    await page.click(card(id), { button: 'right' });
    await page.waitForSelector('#server-group-menu:not([hidden])');
    return page.$$eval('#server-group-menu button', els => els.map(b => b.textContent.trim()));
  };
  const items = await openMenu(nested.id);
  assert(items.includes('复制到同名连接') && items.includes('固定到链接栏'), `menu items: ${items}`);
  await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '复制到同名连接').click());
  // Duplicating now asks first: generate as-is, or generate and edit the clone.
  await page.waitForSelector('#action-dialog[open]');
  const dialogButtons = await page.$$eval('#action-dialog .dialog-buttons button', els => els.filter(el => !el.hidden).map(el => el.textContent.trim()));
  assert.deepEqual(dialogButtons, ['生成后修改', '取消', '直接生成'], `duplicate prompt: ${dialogButtons}`);
  await page.evaluate(() => document.getElementById('action-alt').click());
  await page.waitForSelector('#connection-dialog[open]');
  const editing = await page.evaluate(() => ({ id: document.getElementById('connection-form').elements.id.value, name: document.getElementById('connection-form').elements.name.value, secretNote: !document.getElementById('saved-secret-note').hidden }));
  assert(editing.id && editing.id !== nested.id && editing.name === '子组连接', `editor prefilled: ${JSON.stringify(editing)}`);
  assert(editing.secretNote, 'the clone editor must show the stored-credential hint');
  // Closing the editor without saving still keeps the generated clone.
  await page.evaluate(() => document.getElementById('cancel-connection').click());
  await page.waitForFunction(name => profiles.filter(entry => entry.name === name && !entry.deletedAt).length === 2, {}, '子组连接');
  const clones = (await api('/api/config')).servers.filter(entry => entry.name === '子组连接');
  assert.equal(clones.length, 2);
  assert.notEqual(clones[0].id, clones[1].id);
  assert.equal(clones[0].host, clones[1].host); assert.equal(clones[0].groupId, clones[1].groupId);
  assert.equal(clones.filter(entry => entry.hasSecret).length, 2, 'the clone must carry the stored credential');
  assert(!JSON.stringify(clones).includes('test-secret'), 'the duplicate must never expose the secret in plaintext');
  // 直接生成 creates immediately; 取消 creates nothing.
  await openMenu(kept.id, source.id);
  await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '复制到同名连接').click());
  await page.waitForSelector('#action-dialog[open]');
  await page.evaluate(() => document.getElementById('action-confirm').click());
  await page.waitForFunction(name => profiles.filter(entry => entry.name === name && !entry.deletedAt).length === 2, {}, '留在原组');
  await new Promise(resolve => setTimeout(resolve, 250)); // let the list re-render settle
  await openMenu(kept.id, source.id);
  await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '复制到同名连接').click());
  await page.waitForSelector('#action-dialog[open]');
  await page.evaluate(() => document.getElementById('action-cancel').click());
  await new Promise(resolve => setTimeout(resolve, 300));
  assert.equal((await api('/api/config')).servers.filter(entry => entry.name === '留在原组' && !entry.deletedAt).length, 2, 'cancel must not create a clone');
  // A connection card can be dragged straight onto a left folder row to move it.
  await page.click(folder(target.id));
  await page.waitForSelector(card(nested.id));
  const dragStart = await page.evaluate(id => {
    const cardButton = document.querySelector(`#connection-groups [data-profile-id="${id}"] .connection-card`);
    const draggable = cardButton.draggable;
    cardButton.dispatchEvent(new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() }));
    return { draggable, dragging: (serverManager.profileDrag || { ids: [] }).ids };
  }, nested.id);
  assert(dragStart.draggable, 'saved connection cards must be draggable');
  assert.deepEqual(dragStart.dragging, [nested.id]);
  const dropVisual = await page.evaluate(groupId => {
    const wrap = document.querySelector(`#server-folder-list [data-group-id="${groupId}"]`);
    const transfer = new DataTransfer();
    wrap.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    const highlighted = wrap.classList.contains('server-folder-drop-into');
    wrap.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    return highlighted;
  }, other.id);
  assert(dropVisual, 'the folder row must highlight while a connection is dragged over it');
  await page.waitForFunction((id, groupId) => profiles.find(entry => entry.id === id)?.groupId === groupId, {}, nested.id, other.id);
  assert.equal((await api('/api/config')).servers.find(entry => entry.id === nested.id).groupId, other.id);
  // Dragging a card inside the multi-selection moves the whole selection.
  await new Promise(resolve => setTimeout(resolve, 250)); // let the folder re-render settle
  await page.click(folder(source.id));
  await page.waitForSelector(card(kept.id));
  const multiDrag = await page.evaluate(ids => {
    for (const id of ids) serverManager.selectedProfiles.add(id);
    reflectServerSelection();
    const cardButton = document.querySelector(`#connection-groups [data-profile-id="${ids[0]}"] .connection-card`);
    cardButton.dispatchEvent(new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() }));
    return (serverManager.profileDrag || { ids: [] }).ids.length;
  }, [kept.id, editing.id]);
  assert.equal(multiDrag, 2, 'dragging a selected card must carry the whole selection');
  await page.evaluate(groupId => {
    const wrap = document.querySelector(`#server-folder-list [data-group-id="${groupId}"]`);
    const transfer = new DataTransfer();
    wrap.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    wrap.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  }, parent.id);
  await page.waitForFunction(args => args.ids.every(id => profiles.find(entry => entry.id === id)?.groupId === args.groupId), {}, { ids: [kept.id, editing.id], groupId: parent.id });
  const afterDrag = (await api('/api/config')).servers.filter(entry => [kept.id, editing.id].includes(entry.id));
  assert(afterDrag.length === 2 && afterDrag.every(entry => entry.groupId === parent.id), `multi drag: ${JSON.stringify(afterDrag.map(entry => entry.groupId))}`);
  await new Promise(resolve => setTimeout(resolve, 250)); // let the list re-render settle
  await openMenu(nested.id, other.id);
  await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '固定到链接栏').click());
  await page.waitForFunction(id => (appearance.layout['dengshell.pinned.v1'] || []).includes(id), {}, nested.id);
  const flipItems = await openMenu(nested.id, other.id);
  assert(flipItems.includes('从链接栏取消固定') && !flipItems.includes('固定到链接栏'), `flip items: ${flipItems}`);
  await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '从链接栏取消固定').click());
  await page.waitForFunction(id => !(appearance.layout['dengshell.pinned.v1'] || []).includes(id), {}, nested.id);
  // 删除分组（保留连接）：连接与子分组移交给选定的接管分组。
  await page.click(`#server-folder-list [data-group-id="${parent.id}"] .server-group-more`);
  await page.waitForSelector('#server-group-menu:not([hidden])');
  const groupItems = await page.$$eval('#server-group-menu button', els => els.map(button => button.textContent.trim()));
  assert(groupItems.includes('删除分组（保留连接）') && groupItems.includes('删除分组及全部连接'), `group menu: ${groupItems}`);
  await page.evaluate(() => [...document.querySelectorAll('#server-group-menu button')].find(button => button.textContent === '删除分组（保留连接）').click());
  await page.waitForSelector('#server-group-keep-dialog[open]');
  const keepScope = await page.$eval('#server-group-keep-scope', element => element.textContent);
  assert(keepScope.includes('仅删除分组本身') && keepScope.includes('子分组'), `keep-connections scope: ${keepScope}`);
  const keepOptions = await page.$$eval('#server-group-keep-target option', options => options.map(option => option.value));
  assert(!keepOptions.includes(parent.id) && !keepOptions.includes(target.id) && keepOptions.includes(other.id), `handover options: ${keepOptions}`);
  await page.select('#server-group-keep-target', other.id);
  await page.click('#save-server-group-keep');
  await page.waitForFunction(groupId => !serverManager.nodes.some(node => node.id === groupId), {}, parent.id);
  const afterKeepDelete = await api('/api/config');
  assert(!afterKeepDelete.groupNodes.some(entry => entry.id === parent.id), 'group node must be gone');
  assert(afterKeepDelete.servers.filter(entry => [kept.id, editing.id].includes(entry.id)).every(entry => entry.groupId === other.id), 'profiles must move into the handover group');
  assert(afterKeepDelete.groupNodes.find(entry => entry.id === target.id).parentId === other.id, 'child group must follow into the handover group');
  assert.deepEqual(errors, []);
  console.log('PASS: group filtering, descendant switch, duplicate group paths, move/cancel/error recovery, refreshed counts, selected destination and reload persistence; duplicate-same-name clone (credential-carrying) and links-bar pin/unpin from the profile menu.');
} finally {
  if (browser) await browser.close();
  child.kill();
}
