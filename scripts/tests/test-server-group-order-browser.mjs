// Isolated native drag-and-drop regression; never opens real connections.
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
const a = await api('/api/group-nodes', { name: 'Alpha' });
const z = await api('/api/group-nodes', { name: 'Zulu' });
const c = await api('/api/group-nodes', { name: 'Child C', parentId: a.id });
const d = await api('/api/group-nodes', { name: 'Child D', parentId: a.id });
const profile = await api('/api/profiles', { name: '连接保留', groupId: c.id, user: 'root', host: '192.0.2.1', auth: 'password', secret: 'fixture-secret' });
const profileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'devshell-group-order-'));
const child = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--noerrdialogs', `--user-data-dir=${profileDir}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore', windowsHide: true });
let browser;
try {
  let wsEndpoint;
  for (let attempt = 0; attempt < 100 && !wsEndpoint; attempt++) {
    const portFile = path.join(profileDir, 'DevToolsActivePort');
    if (fs.existsSync(portFile)) { const [port, wsPath] = fs.readFileSync(portFile, 'utf8').trim().split(/\r?\n/); if (port && wsPath) wsEndpoint = `ws://127.0.0.1:${port.trim()}${wsPath.trim()}`; }
    if (!wsEndpoint) await new Promise(resolve => setTimeout(resolve, 200));
  }
  assert(wsEndpoint, 'headless browser is available');
  browser = await puppeteer.connect({ browserWSEndpoint: wsEndpoint });
  const page = await browser.newPage(), errors = [], orders = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (request.url().endsWith('/api/group-nodes/order')) orders.push(JSON.parse(request.postData())); });
  await page.setViewport({ width: 1440, height: 1000 });
  await page.goto(fixture.url, { waitUntil: 'networkidle0' });
  await page.evaluate(async id => { await loadProfiles(); setDrawer(true); chooseServerFolder(id); }, a.id);
  const row = id => `#server-folder-list [data-group-id="${id}"]`;
  const select = id => `${row(id)} .server-folder-select`;
  const listed = () => page.$$eval('#server-folder-list [data-group-id]', rows => rows.map(row => row.dataset.groupId));
  const customOrder = async () => (await listed()).filter(id => [a.id, z.id, c.id, d.id].includes(id));
  const drag = async (fromID, toID, before) => {
    const from = await page.$eval(select(fromID), el => { const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
    const to = await page.$eval(`${row(toID)} .server-folder-row`, (el, before) => { const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height * (before ? .2 : .8) }; }, before);
    await page.mouse.dragAndDrop(from, to, { delay: 60 });
    await page.waitForFunction(() => !serverManager.groupOrderSaving);
  };
  await page.setDragInterception(true);
  assert.deepEqual(await customOrder(), [a.id, c.id, d.id, z.id]);
  assert.equal(await page.$eval(select('__ungrouped__'), el => el.draggable), false, 'the ungrouped filter row is fixed');
  await drag(z.id, a.id, true);
  assert.deepEqual(await customOrder(), [z.id, a.id, c.id, d.id]);
  assert.equal(await page.evaluate(() => serverManager.selectedGroup), a.id, 'sorting preserves selection');
  await drag(d.id, c.id, true);
  assert.deepEqual(await customOrder(), [z.id, a.id, d.id, c.id]);
  assert.equal((await api('/api/config')).servers.find(p => p.id === profile.id).groupId, c.id, 'sorting preserves connection membership');
  const saved = (await api('/api/config')).groupNodes;
  assert(saved.find(g => g.id === z.id).order < saved.find(g => g.id === a.id).order);
  assert(saved.find(g => g.id === d.id).order < saved.find(g => g.id === c.id).order);
  const requests = orders.length;
  await drag(d.id, z.id, true);
  assert.equal(orders.length, requests, 'cross-parent drop is rejected');
  assert.deepEqual(await customOrder(), [z.id, a.id, d.id, c.id]);
  // Cancel an intercepted native drag without writing an order.
  const source = await page.$eval(select(a.id), el => { const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
  const data = await page.mouse.drag(source, { x: source.x + 20, y: source.y + 10 });
  const client = await page.createCDPSession();
  await client.send('Input.dispatchDragEvent', { type: 'dragCancel', x: source.x, y: source.y, data });
  await page.mouse.up(); await client.detach();
  assert.equal(orders.length, requests);
  await page.setDragInterception(false);
  await page.click(select(d.id));
  await page.keyboard.down('Alt'); await page.keyboard.press('ArrowDown'); await page.keyboard.up('Alt');
  await page.waitForFunction(() => !serverManager.groupOrderSaving);
  assert.deepEqual(await customOrder(), [z.id, a.id, c.id, d.id], 'keyboard order uses the same saved sequence');
  assert.equal(await page.$eval(select(d.id), el => document.activeElement === el), true, 'keyboard focus survives rendering');
  // A rejected save keeps the previously rendered order and allows retry.
  let reject = true;
  await page.setRequestInterception(true);
  page.on('request', request => reject && request.url().endsWith('/api/group-nodes/order') ? request.respond({ status: 400, contentType: 'application/json', body: JSON.stringify({ error: '模拟保存失败' }) }) : request.continue());
  await page.setDragInterception(true);
  await drag(a.id, z.id, true);
  assert.deepEqual(await customOrder(), [z.id, a.id, c.id, d.id]);
  assert.equal(await page.$$eval('.server-folder-drop-before,.server-folder-drop-after,.server-folder-dragging', rows => rows.length), 0, 'drop indicators clear on error');
  reject = false;
  await drag(a.id, z.id, true);
  assert.deepEqual(await customOrder(), [a.id, c.id, d.id, z.id], 'a failed save does not block another drag');
  await api('/api/group-nodes', { ...z, name: 'AAA renamed' });
  await page.reload({ waitUntil: 'networkidle0' });
  await page.evaluate(async () => { await loadProfiles(); setDrawer(true); });
  assert.deepEqual(await customOrder(), [a.id, c.id, d.id, z.id], 'rename and reload preserve manual order');
  await page.screenshot({ path: path.join(stage, 'server-group-order.png') });
  assert.deepEqual(errors, []);
  console.log('PASS: native sibling drag before/after, subtree membership, cancel, invalid target, keyboard/focus, failed-save retry, rename and reload persistence.');
} finally {
  if (browser) await browser.close();
  child.kill();
}
