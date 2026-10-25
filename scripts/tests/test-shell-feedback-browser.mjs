// Disposable local backend only. Native resize is simulated with viewport changes.
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const [url, modulePath] = process.argv.slice(2);
assert.equal(new URL(url).hostname, '127.0.0.1');
const { chromium } = await import(pathToFileURL(modulePath).href);
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(url);
  await page.waitForFunction(() => document.querySelector('#connection-button').title === '打开服务器管理');
  await page.evaluate(() => { document.querySelectorAll('dialog[open]').forEach(dialog => dialog.close()); setDrawer(false); });
  const settle = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  for (const width of [1100, 1440, 640]) {
    await page.setViewportSize({ width, height: 900 });
    for (const scale of [1, 1.5]) {
      await page.evaluate(scale => { appearance.uiScale = scale; appearance.filesPosition = 'right'; setWorkspaceVisible(true, false); applyUIScale(); $('#detach-terminal').hidden = false; }, scale);
      await settle();
      const controls = await page.evaluate(() => [...document.querySelectorAll('.terminal-meta button')].filter(button => !button.hidden).map(button => {
        const r = button.getBoundingClientRect(), panel = button.closest('.terminal-panel').getBoundingClientRect();
        return { id: button.id, width: r.width, inside: r.left >= panel.left && r.right <= panel.right + 1, hit: button.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)) };
      }));
      for (const control of controls) assert(control.width > 0 && control.inside && control.hit, `${width}/${scale}: ${JSON.stringify(control)}`);
    }
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() => { appearance.uiScale = 1; setWorkspaceVisible(false, false); applyUIScale(); });
  const calls = []; let growth = 0;
  await page.exposeFunction('resizeForFiles', async (open, width) => {
    calls.push({ open, width });
    const size = page.viewportSize();
    await page.setViewportSize({ width: size.width + (open ? width : -growth), height: size.height });
    growth = open ? width : 0;
  });
  await page.evaluate(() => { window.go = { desktop: { Desktop: { SetFilesPanel: window.resizeForFiles } } }; });
  const terminalWidth = () => page.locator('.terminal-panel').evaluate(el => el.getBoundingClientRect().width);
  const before = await terminalWidth();
  await page.click('#toggle-sftp'); await page.evaluate(() => rightPanelResize); await settle();
  assert(Math.abs(await terminalWidth() - before) < 2, 'opening right SFTP preserves terminal width');
  assert.equal(page.viewportSize().width, 1868);
  await page.click('#toggle-sftp'); await page.evaluate(() => rightPanelResize); await settle();
  assert.equal(page.viewportSize().width, 1440);
  await page.evaluate(() => { setWorkspaceVisible(true); setWorkspaceVisible(false); setWorkspaceVisible(true); });
  await page.evaluate(() => rightPanelResize); await settle();
  assert.equal(page.viewportSize().width, 1868, 'rapid toggles do not accumulate growth');
  await page.evaluate(() => { appearance.filesPosition = 'bottom'; applyUIScale(); });
  await page.evaluate(() => rightPanelResize); await settle();
  assert.equal(page.viewportSize().width, 1440, 'placement change releases right panel space');
  await page.evaluate(() => {
    setWorkspaceVisible(false); localTasks.clear();
    localTasks.set('qa', { id: 'qa', name: 'rz-upload.txt', total: 100, done: 35, status: 'uploading' });
    updateTransferNotice();
  });
  assert.match(await page.locator('#shell-transfer-status').textContent(), /上传中 · 35%/);
  assert(await page.locator('#shell-transfer-status').isVisible());
  if (process.env.DENGSHELL_FEEDBACK_SCREENSHOT) await page.screenshot({ path: process.env.DENGSHELL_FEEDBACK_SCREENSHOT });
  await page.evaluate(() => { localTasks.get('qa').status = 'done'; updateTransferNotice(); });
  assert.equal(await page.locator('#toast').textContent(), '上传已完成');
  await page.locator('#shell-transfer-status').click();
  assert(await page.locator('#transfers-view').isVisible());
  assert.equal(await page.locator('#shell-transfer-status').isVisible(), false);
  assert.deepEqual(errors, []);
  console.log('PASS: toolbar visibility, right expansion, rapid toggles, placement changes, hidden-SFTP upload feedback', calls);
} finally { await browser.close(); }
