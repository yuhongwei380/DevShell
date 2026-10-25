// Use a disposable localhost app; no real SSH connections or user settings.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
const [stage, chrome, modulePath] = process.argv.slice(2);
const { default: puppeteer } = await import(path.isAbsolute(modulePath) ? pathToFileURL(modulePath).href : modulePath);
const fixture = JSON.parse(fs.readFileSync(path.join(stage, 'browser-fixture.json')));
const setup = await fetch(new URL('/api/appearance/patch', fixture.url), {
  method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': fixture.token },
  body: JSON.stringify({ onboardingCompleted: true, uiScale: 1, filesPosition: 'right', monitorVisible: true, layout: { 'cloudshell.layout': { version: 2, sidebar: 450, filesWidth: 650, files: 210 } } }),
});
assert(setup.ok, 'legacy width fixture saved');
const profileDir = fs.mkdtempSync(path.join(stage, 'edge-fixed-panels-'));
const child = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--noerrdialogs', `--user-data-dir=${profileDir}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore', windowsHide: true });
let browser;
try {
  let wsEndpoint;
  for (let attempt = 0; attempt < 100 && !wsEndpoint; attempt++) {
    const portFile = path.join(profileDir, 'DevToolsActivePort');
    if (fs.existsSync(portFile)) { const [port, wsPath] = fs.readFileSync(portFile, 'utf8').trim().split(/\r?\n/); if (port && wsPath) wsEndpoint = `ws://127.0.0.1:${port.trim()}${wsPath.trim()}`; }
    if (!wsEndpoint) await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert(wsEndpoint, 'headless browser is available');
  browser = await puppeteer.connect({ browserWSEndpoint: wsEndpoint });
  const page = await browser.newPage(), errors = [], measurements = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.setViewport({ width: 1440, height: 1000 });
  await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
  await page.goto(fixture.url, { waitUntil: 'networkidle0' });
  const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const snapshot = () => page.evaluate(() => ({
    monitor: $('.sidebar').offsetWidth, files: $('#files-panel').clientWidth, height: $('#files-panel').clientHeight,
    terminal: $('.terminal-panel').clientWidth, workspace: $('#workspace').clientWidth,
    narrow: document.documentElement.dataset.narrow === 'true', scale: effectiveScale,
    overflow: $('#workspace').scrollWidth > $('#workspace').clientWidth + 1,
    pageOverflow: document.body.scrollWidth > document.body.clientWidth + 1,
    gap: { cursor: getComputedStyle($('#files-splitter')).cursor, tabIndex: $('#files-splitter').tabIndex },
  }));
  const fixed = async label => {
    await settle(); const state = await snapshot(); measurements.push({ label, ...state });
    if (!state.narrow) assert.equal(state.monitor, 304, label + ': monitor is fixed');
    assert.equal(state.files, Math.min(420, Math.max(160, state.workspace - 248)), label + ': fixed SFTP with small-window constraint');
    assert(!state.overflow && !state.pageOverflow, label + ': no horizontal overflow');
    assert(state.terminal >= 239, label + ': usable terminal space remains');
    assert.equal(state.gap.cursor, 'default'); assert.equal(state.gap.tabIndex, -1);
    return state;
  };
  const drag = async (selector, dx, dy) => {
    const box = await page.$eval(selector, element => { const r = element.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
    await page.mouse.move(box.x, box.y); await page.mouse.down();
    await page.mouse.move(box.x + dx, box.y + dy, { steps: 6 }); await page.mouse.up(); await settle();
  };
  await page.evaluate(() => { setDrawer(false); setWorkspaceVisible(true, false); });
  assert.equal((await fixed('legacy widths ignored')).files, 420);
  await drag('#sidebar-splitter', 100, 0); await fixed('monitor drag ignored');
  assert.equal(await page.$eval('#sidebar-splitter', el => el.hasAttribute('tabindex')), false);
  await drag('#files-splitter', -100, 0); await fixed('right drag ignored');
  await page.focus('#files-splitter'); await page.keyboard.press('ArrowLeft'); await fixed('right keyboard resize ignored');
  for (const width of [1920, 1280, 1100, 1000, 960, 800, 640]) {
    await page.setViewport({ width, height: 1000 }); await fixed('viewport ' + width);
    if (width === 640) {
      await page.click('#toggle-monitor');
      assert.equal((await snapshot()).monitor, 304, 'small-window monitor drawer keeps fixed width');
      await page.click('#monitor-backdrop');
    }
  }
  await page.setViewport({ width: 1600, height: 1000 });
  for (const scale of [.75, 1, 1.25, 1.5]) {
    await page.evaluate(scale => { appearance.uiScale = scale; applyUIScale(); }, scale);
    assert.equal((await fixed('UI scale ' + scale)).files, 420);
  }
  await page.evaluate(async () => { await chooseAppearance({ uiScale: 1, monitorVisible: false }); });
  await settle();
  assert.equal(await page.$eval('.sidebar', el => el.getBoundingClientRect().width), 0, 'monitor hidden by appearance setting');
  const hiddenMonitor = await snapshot(); assert(!hiddenMonitor.overflow && !hiddenMonitor.pageOverflow, 'hidden monitor: no overflow');
  assert(hiddenMonitor.terminal > 1200, 'hidden monitor: terminal takes the released column');
  await page.evaluate(async () => { await chooseAppearance({ monitorVisible: true }); });
  await fixed('monitor shown again');
  const beforeHide = await fixed('before hiding SFTP');
  await page.click('#toggle-sftp'); await settle();
  const hidden = await snapshot(); assert.equal(hidden.terminal, hidden.workspace); assert(hidden.terminal > beforeHide.terminal);
  await page.click('#toggle-sftp'); await fixed('SFTP shown again');
  for (const pane of ['files', 'transfers', 'commands', 'common-apps']) {
    await page.click(`.file-tab[data-pane="${pane}"]`); await fixed(pane + ' card on right');
  }
  for (const position of ['bottom', 'top']) {
    await page.evaluate(async filesPosition => { await chooseAppearance({ filesPosition }); }, position); await settle();
    assert.equal(await page.$eval('#files-splitter', el => el.tabIndex), 0);
    const before = await snapshot();
    await page.focus('#files-splitter'); await page.keyboard.press(position === 'top' ? 'ArrowDown' : 'ArrowUp'); await settle();
    assert((await snapshot()).height > before.height + 10, position + ': keyboard adjusts height');
    const beforeDrag = await snapshot(); await drag('#files-splitter', 0, position === 'top' ? 35 : -35);
    assert((await snapshot()).height > beforeDrag.height + 25, position + ': pointer adjusts height');
  }
  await page.evaluate(async () => { await chooseAppearance({ filesPosition: 'right' }); await DengPortablePreferences.flush(); });
  await page.reload({ waitUntil: 'networkidle0' }); await fixed('reload');
  await page.click('.file-tab[data-pane="files"]');
  const output = process.env.DENGSHELL_LAYOUT_QA_DIR || stage; fs.mkdirSync(output, { recursive: true });
  for (const theme of ['light', 'dark']) {
    await page.evaluate(theme => DevShellTheme.set(theme), theme); await settle(); await fixed(theme + ' theme');
    await page.screenshot({ path: path.join(output, 'fixed-panels-' + theme + '.png') });
  }
  assert.deepEqual(errors, []);
  fs.writeFileSync(path.join(output, 'fixed-panel-widths-results.json'), JSON.stringify({ passed: true, measurements, errors }, null, 2));
  console.log('PASS: fixed monitor/right SFTP widths, stale preferences, resize/zoom, disabled width handles, narrow layout, height resizing, hide/show, all cards, themes and reload.');
} finally {
  if (browser) await browser.close();
  child.kill();
}
