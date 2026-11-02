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
  assert.deepEqual(errors, []);
  console.log('PASS: group filtering, descendant switch, duplicate group paths, move/cancel/error recovery, refreshed counts, selected destination and reload persistence.');
} finally {
  if (browser) await browser.close();
  child.kill();
}
