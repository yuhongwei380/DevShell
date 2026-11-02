// Browser fixture for the local shell feature (issue #4 slice A).
// Windows: the feature is removed — the 设置 → 其他功能 row is hidden, the
// switch cannot be turned on (backend rejects it), and “+” always opens the
// start tab. Other supported platforms (macOS shipped, Linux for CI): the
// switch defaults to 关闭, turning it on makes “+” open a real local terminal
// with a typing round-trip, the tab is marked · 本机, exit disconnects and the
// switch persists across reload. In every case the “+” button reads 新会话.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const [stage, chrome, modulePath] = process.argv.slice(2);
const { default: puppeteer } = await import(modulePath);
const fixture = JSON.parse(fs.readFileSync(stage + '/browser-fixture.json'));
const origin = new URL(fixture.url).origin;
const headers = { 'Content-Type': 'application/json', 'X-CloudShell-Token': fixture.token };
const setup = await fetch(origin + '/api/appearance/patch', { method: 'POST', headers, body: JSON.stringify({ onboardingCompleted: true, uiScale: 1, theme: 'dark', localShellEnabled: false }) });
assert(setup.ok, 'fixture setup must clear the local shell switch');
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage'] });
const errors = [], checks = [];
try {
  const page = await browser.newPage(); page.on('pageerror', e => errors.push(e.message));
  await page.setViewport({ width: 1440, height: 1000 });
  await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
  await page.goto(fixture.url, { waitUntil: 'networkidle0' });
  await page.evaluate(() => { document.querySelectorAll('dialog[open]').forEach(d => d.close()); pollStats = pollNetwork = pollLatency = () => {}; });

  const supported = await page.evaluate(() => window.DEVSHELL?.localShellSupported !== false);
  const rowHidden = await page.evaluate(() => {
    const settings = document.querySelector('#settings-button'); settings.click();
    document.querySelector('#manage-other-features').click();
    const row = document.querySelector('#feature-local-shell')?.closest('.placement-setting');
    return !row || row.hidden;
  });
  await page.waitForFunction(() => !document.querySelector('#settings-other-view').hidden);

  // The affordance rename holds on every platform.
  assert.equal(await page.$eval('#add-session', el => el.title), '新会话');
  assert.equal(await page.$eval('#add-session', el => el.getAttribute('aria-label')), '新会话');

  if (!supported) {
    // 1) Windows: the row is gone, the API refuses to enable the switch and
    // refuses to spawn a session, and “+” keeps the start-tab behavior.
    assert(rowHidden, 'the 本地终端 row must be hidden on Windows');
    const patch = await fetch(origin + '/api/appearance/patch', { method: 'POST', headers, body: JSON.stringify({ localShellEnabled: true }) });
    assert(!patch.ok, 'Windows must reject turning the switch on');
    const spawn = await fetch(origin + '/api/local-shell/sessions', { method: 'POST', headers, body: '{}' });
    assert(!spawn.ok, 'Windows must refuse local shell sessions');
    checks.push({ platform: 'windows', rowHidden: true, switchRejected: true, spawnRejected: true });
    await page.click('#other-settings-back');
    await page.evaluate(() => setSettingsMenu(false));
    await page.click('#add-session');
    await page.waitForFunction(() => !!window.DengStartTabs.first(), { timeout: 5000 });
    assert.equal(await page.evaluate(() => [...sessions.values()].some(s => s.localShell)), false);
    checks.push({ offBehavior: 'start-tab' });
    assert.deepEqual(errors, []);
    fs.writeFileSync(stage + '/local-shell-result.json', JSON.stringify({ passed: true, checks, errors }, null, 2));
    console.log('PASS (windows): 本地终端 row hidden, switch and spawn API rejected, “+” stays 新会话 start-tab, no page errors');
  } else {
    // 2) Supported platforms: default 关闭, real session once on.
    assert.equal(await page.$eval('#feature-local-shell', el => el.value), 'false', 'the local shell switch must default to 关闭');
    checks.push({ menu: 'other-features', default: 'off' });
    await page.click('#other-settings-back');
    await page.evaluate(() => setSettingsMenu(false));
    await page.click('#add-session');
    await page.waitForFunction(() => !!window.DengStartTabs.first(), { timeout: 5000 });
    assert.equal(await page.evaluate(() => [...sessions.values()].some(s => s.localShell)), false);
    checks.push({ offBehavior: 'start-tab' });
    await page.evaluate(() => window.DengStartTabs.close(window.DengStartTabs.first()));

    await page.select('#feature-local-shell', 'true');
    await page.evaluate(() => appearanceSave);
    assert.equal(await page.evaluate(() => appearance.localShellEnabled), true);
    await page.click('#add-session');
    await page.waitForFunction(() => [...sessions.values()].some(s => s.localShell && s.connected), { timeout: 30000 });
    await page.waitForFunction(() => current()?.ready && current()?.localShell, { timeout: 15000 });
    const ready = await page.evaluate(() => { const s = current(); return { id: s.id, shell: s.shell, name: s.name, cwd: s.cwd }; });
    assert(ready.shell && ready.name, 'the backend must report the shell it started');
    checks.push({ opened: true, shell: ready.shell, cwd: ready.cwd });

    // Typing round-trips; proof of a live shell is behavioral: the marker
    // surfaces and `exit` below terminates the process and disconnects.
    await page.evaluate(() => sendInput(current(), 'echo LOCAL-SHELL-E2E\r'));
    await page.waitForFunction(() => {
      const b = current()?.term?.buffer?.active; if (!b) return false;
      let text = ''; for (let i = 0; i < b.length; i++) text += b.getLine(i)?.translateToString(true) || '';
      return text.includes('LOCAL-SHELL-E2E');
    }, { timeout: 20000 });
    checks.push({ typed: true });

    const tab = await page.$eval('.session-tab button', el => el.title);
    assert(tab.includes('· 本机'), `local tab must be marked 本机: ${tab}`);
    assert.equal(await page.evaluate(() => profiles.length), 0, 'the pseudo profile must never reach the store');
    checks.push({ tabTitle: tab });

    const sessionID = ready.id;
    await page.evaluate(() => sendInput(current(), 'exit\r'));
    await page.waitForFunction(id => sessions.get(id) && !sessions.get(id).connected, { timeout: 20000 }, sessionID);
    checks.push({ exitDisconnect: true });

    await page.reload({ waitUntil: 'networkidle0' });
    await page.evaluate(() => { document.querySelectorAll('dialog[open]').forEach(d => d.close()); });
    assert.equal(await page.evaluate(() => appearance.localShellEnabled), true);
    await page.click('#settings-button'); await page.click('#manage-other-features');
    await page.waitForFunction(() => !document.querySelector('#settings-other-view').hidden);
    await page.select('#feature-local-shell', 'false');
    await page.evaluate(() => appearanceSave);
    await page.reload({ waitUntil: 'networkidle0' });
    assert.equal(await page.evaluate(() => appearance.localShellEnabled), false);
    checks.push({ persisted: true });

    assert.deepEqual(errors, []);
    fs.writeFileSync(stage + '/local-shell-result.json', JSON.stringify({ passed: true, checks, errors }, null, 2));
    console.log('PASS: 其他功能 switch defaults 关闭 and persists; “+” opens a real local shell once on (echo round-trip, exit disconnects); tab marked · 本机; pseudo profile never stored');
  }
} finally { await browser.close(); }
