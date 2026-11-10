// Real import -> preferences API -> xterm DOM regression, using an isolated
// TestFontLibraryBrowserFixture. Arguments: fixture directory, Edge/Chrome,
// absolute path to a Playwright index.mjs (no extra repository dependency).
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const [stage, executablePath, modulePath] = process.argv.slice(2);
const { chromium } = await import(pathToFileURL(modulePath));
const fixture = JSON.parse(fs.readFileSync(path.join(stage, 'browser-fixture.json')));
const binary = fs.readFileSync(new URL('./fixtures/iterm-colors-binary.b64', import.meta.url), 'utf8').trim();
const browser = await chromium.launch({ executablePath, headless: true, args: ['--no-sandbox', '--disable-gpu'] });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' }), errors = [];
page.on('pageerror', error => errors.push(error.message));
const escape = text => String(text).replaceAll('&', '&amp;').replaceAll('<', '&lt;');
const value = item => item === null ? '<data/>' : typeof item === 'number' ? `<real>${item}</real>` : typeof item === 'string' ? `<string>${escape(item)}</string>` : `<dict>${Object.entries(item).map(([k, v]) => `<key>${escape(k)}</key>${value(v)}`).join('')}</dict>`;
const xml = preset => `<?xml version="1.0"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0">${value(preset)}</plist>`;
const color = (r, g, b, space = 'sRGB', alpha) => ({ 'Red Component': r, 'Green Component': g, 'Blue Component': b, 'Color Space': space, ...(alpha === undefined ? {} : { 'Alpha Component': alpha }) });
const base = Object.fromEntries(Array.from({ length: 16 }, (_, i) => [`Ansi ${i} Color`, color(i / 16, (15 - i) / 16, .5)]));
Object.assign(base, {
  'Foreground Color': color(.8, .7, .6), 'Background Color': color(.1, .2, .3),
  'Cursor Color': color(.9, .8, .7), 'Cursor Text Color': color(.2, .3, .4),
  'Selection Color': color(.3, .4, .5), 'Selected Text Color': color(.6, .7, .8),
  'Bold Color': color(.9, .2, .3), 'Underline Color': color(.2, .9, .4), 'Link Color': color(.3, .4, .9),
  'Tab Color': color(.1, .1, .1), 'Match Color': color(.8, .8, .2),
  'Badge Color': color(.8, 0, 0, 'sRGB', .5), 'Cursor Guide Color': color(.1, .2, .3),
});
const parse = preset => page.evaluate(text => DevShellItermColors.parse(text), xml(preset));
const importPreset = async (name, buffer) => {
  await page.locator('#iterm-picker').setInputFiles({ name: `${name}.itermcolors`, mimeType: 'application/octet-stream', buffer });
  await page.waitForFunction(name => appearance.ansiScheme === name && document.querySelector('.scheme-row[data-name="' + name + '"]'), name);
  assert.equal(await page.locator('#scheme-error').textContent(), '');
};
try {
  const response = await fetch(new URL('/api/appearance/patch', fixture.url), { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': fixture.token }, body: JSON.stringify({ onboardingCompleted: true, theme: 'dark', uiScale: 1, ansiPalette: 'soft', ansiSchemes: [], ansiScheme: '' }) });
  assert(response.ok);
  await page.goto(fixture.url, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => typeof chooseAppearance === 'function' && typeof appearance === 'object' && typeof profiles !== 'undefined');
  const corpus = path.join(stage, 'corpus');
  if (fs.existsSync(corpus)) for (const file of fs.readdirSync(corpus).filter(file => file.endsWith('.itermcolors'))) { const preset = await page.evaluate(text => DevShellItermColors.parse(text), fs.readFileSync(path.join(corpus, file), 'utf8')); assert.equal(Object.keys(preset.colors).filter(key => ['black', 'red', 'brightWhite'].includes(key)).length, 3, file); }
  for (const bytes of [Buffer.concat([Buffer.from([255, 254]), Buffer.from(xml(base), 'utf16le')]), Buffer.concat([Buffer.from([254, 255]), Buffer.from(xml(base), 'utf16le').swap16()])]) {
    assert.equal((await page.evaluate(bytes => DevShellItermColors.parse(new Uint8Array(bytes)), [...bytes])).colors.foreground, '#ccb399');
  }
  const parsed = await parse(base);
  assert.equal(Object.keys(parsed.colors).length, 29);
  assert.equal(parsed.colors.foreground, '#ccb399');
  assert.equal(parsed.colors.background, '#1a334d');
  assert.equal(parsed.colors.cursorGuide, '#1a334d40');
  assert.equal(parsed.colors.badge, '#cc000080');
  const calibrated = await parse({ 'Foreground Color': color(.5, .5, .5, 'Calibrated') });
  assert.equal(calibrated.colors.foreground, '#929292');
  assert.equal((await parse({ 'Foreground Color': { 'Red Component': .5, 'Green Component': .5, 'Blue Component': .5 } })).colors.foreground, '#929292');
  const p3Reference = await page.evaluate(() => { const canvas = document.createElement('canvas'), context = canvas.getContext('2d'); context.fillStyle = 'color(display-p3 .5 .2 .7)'; context.fillRect(0, 0, 1, 1); return '#' + [...context.getImageData(0, 0, 1, 1).data].slice(0, 3).map(v => v.toString(16).padStart(2, '0')).join(''); });
  assert.equal((await parse({ 'Foreground Color': color(.5, .2, .7, 'P3') })).colors.foreground, p3Reference);
  assert.equal((await parse({ 'Background Color': color(0, 0, 0, 'sRGB', 0) })).colors.background, '#00000000');
  assert.equal((await parse({ 'Background Color': color(0, 0, 0, 'sRGB', 1) })).colors.background, '#000000');
  const fromBinary = await page.evaluate(binary => DevShellItermColors.parse(Uint8Array.from(atob(binary), c => c.charCodeAt(0))), binary);
  assert.equal(fromBinary.colors.foreground, '#4080bf80');
  assert.equal(fromBinary.lightColors.red, '#4080bf80');
  assert.equal(fromBinary.darkColors.bold, '#4080bf80');
  assert.equal((await parse({ 'Foreground Color': color(.5, .5, .5, 'legacy') })).colors.foreground, '#929292');
  for (const preset of [{}, { ...base, 'Ansi 1 Color': color(NaN, 0, 0) }, { ...base, 'Foreground Color': { 'Red Component': null, 'Green Component': 0, 'Blue Component': 0 } }]) {
    const rejected = await page.evaluate(text => { try { DevShellItermColors.parse(text); return false; } catch { return true; } }, xml(preset));
    assert(rejected, 'malformed colors must not be silently imported');
  }
  for (const text of ['<plist><dict><key>Foreground Color</key></dict></plist>', '<plist><dict><key>x</key><real>1</real><key>x</key><real>2</real></dict></plist>', '<plist><dict>', 'not a plist']) {
    assert(await page.evaluate(text => { try { DevShellItermColors.parse(text); return false; } catch { return true; } }, text));
  }
  assert(await page.evaluate(binary => { const b = Uint8Array.from(atob(binary), c => c.charCodeAt(0)); b[b.length - 1] = 255; try { DevShellItermColors.parse(b); return false; } catch { return true; } }, binary));
  await page.evaluate(async () => {
    await chooseAppearance({ onboardingCompleted: true });
    document.querySelectorAll('dialog[open]').forEach(d => d.close()); setDrawer(false); pollStats = () => {}; pollNetwork = () => {};
    const id = 'iterm-colors-qa'; profiles.push({ id, name: 'iTerm2 配色验证', host: 'fixture.invalid', user: 'qa', port: 22 });
    const s = { ...makeSessionState({ id, profileId: id, home: '/' }), localOnly: true };
    sessions.set(id, s); createTerminal(s); activate(id); s.fit.fit = () => {};
    window.itermQAWrite = text => new Promise(resolve => s.term.write(text, resolve));
    window.itermQARender = async () => {
      s.term.reset(); s.term.resize(90, 30);
      await itermQAWrite('\x1b[?25lNORMAL\r\n\x1b[1mBOLD\r\n\x1b[31mANSI BOLD\r\n\x1b[38;2;17;34;51mTRUECOLOR\r\n\x1b[0;4mUNDERLINE\r\n\x1b[0;7mINVERSE\r\n\x1b[0m\x1b]8;;https://example.test\x07LINK\x1b]8;;\x07\r\n中文 ┌─┐\x1b[0m');
      s.term.refresh(0, s.term.rows - 1);
    };
  });
  await page.locator('#settings-button').click();
  await page.evaluate(() => document.querySelector('#manage-ansi-schemes').click());
  await importPreset('Full', Buffer.from(xml(base)));
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').close());
  await page.evaluate(() => itermQARender());
  await page.waitForTimeout(100);
  const rendering = await page.evaluate(() => ({
    theme: current().term.options.theme,
    rows: [...current().host.querySelectorAll('.xterm-rows > div')].slice(0, 7).map(row => { const style = getComputedStyle(row.querySelector('span')); return { color: style.color, underline: style.textDecorationColor }; }),
    background: getComputedStyle(document.querySelector('#terminal-output')).backgroundColor,
    scroll: getComputedStyle(current().host.querySelector('.xterm-scrollable-element')).backgroundColor,
  }));
  assert.equal(rendering.background, 'rgba(0, 0, 0, 0)');
  assert.equal(rendering.theme.background, '#00000000');
  assert.equal(await page.locator('#scheme-background-toggle').count(), 0);
  assert.equal(rendering.scroll, 'rgba(0, 0, 0, 0)');
  assert.equal(rendering.rows[0].color, 'rgb(204, 179, 153)');
  assert.equal(rendering.rows[1].color, 'rgb(230, 51, 77)');
  assert.equal(rendering.rows[2].color, 'rgb(143, 96, 128)');
  assert.equal(rendering.rows[3].color, 'rgb(17, 34, 51)');
  assert.equal(rendering.rows[4].underline, 'rgb(51, 230, 102)');
  assert.equal(rendering.rows[5].color, 'rgb(0, 0, 0)');
  assert.equal(rendering.rows[6].color, 'rgb(77, 102, 230)');
  await page.evaluate(async () => { const t = current().term; t.select(0, 1, 4); });
  await page.waitForTimeout(80);
  assert.equal(await page.evaluate(() => getComputedStyle(current().host.querySelectorAll('.xterm-rows > div')[1].querySelector('span')).color), 'rgb(153, 179, 204)');
  await page.evaluate(() => current().term.clearSelection());
  await page.screenshot({ path: path.join(stage, 'iterm-full.png') });
  await page.evaluate(async () => itermQAWrite('\x1b]11;rgb:11/22/33\x07'));
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#terminal-output')).backgroundColor), 'rgb(17, 34, 51)');
  await page.evaluate(() => {
    window.itermOriginalID = current().id;
    const id = 'iterm-colors-second'; profiles.push({ id, name: '第二个终端', host: 'fixture.invalid', user: 'qa', port: 22 });
    const s = { ...makeSessionState({ id, profileId: id, home: '/' }), localOnly: true }; sessions.set(id, s); createTerminal(s); activate(id);
  });
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#terminal-output')).backgroundColor), 'rgba(0, 0, 0, 0)');
  await page.evaluate(() => activate(itermOriginalID));
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#terminal-output')).backgroundColor), 'rgb(17, 34, 51)');
  await page.evaluate(async () => itermQAWrite('\x1b]111\x07'));
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#terminal-output')).backgroundColor), 'rgba(0, 0, 0, 0)');
  const dracula = path.join(stage, 'corpus', 'Dracula.itermcolors');
  if (fs.existsSync(dracula)) {
    await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').showModal());
    await importPreset('Dracula', fs.readFileSync(dracula));
    await page.evaluate(async () => { document.querySelector('#ansi-schemes-dialog').close(); await chooseAppearance({ uiStyle: 'frosted' }); await itermQARender(); });
    assert.equal(await page.evaluate(() => current().term.options.theme.background), '#00000000');
    await page.screenshot({ path: path.join(stage, 'iterm-dracula.png') });
  }
  // A later invalid file must roll back the entire multi-file import.
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').showModal());
  const beforeInvalid = await page.evaluate(() => JSON.stringify(appearance.ansiSchemes));
  await page.locator('#iterm-picker').setInputFiles([
    { name: 'Transient.itermcolors', mimeType: 'application/xml', buffer: Buffer.from(xml(base)) },
    { name: 'Broken.itermcolors', mimeType: 'application/xml', buffer: Buffer.from(xml({ 'Foreground Color': color(NaN, 0, 0) })) },
  ]);
  await page.waitForFunction(() => document.querySelector('#scheme-error').textContent.length > 0);
  assert.equal(await page.evaluate(() => JSON.stringify(appearance.ansiSchemes)), beforeInvalid);
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').close());
  const dual = { ...base, 'Foreground Color (Light)': color(.1, .1, .1), 'Background Color (Light)': color(.9, .9, .9), 'Foreground Color (Dark)': color(.9, .9, .9), 'Background Color (Dark)': color(.1, .1, .1) };
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').showModal());
  await importPreset('Dual', Buffer.from(xml(dual)));
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').close());
  for (const theme of ['light', 'dark']) {
    await page.evaluate(theme => DevShellTheme.set(theme), theme);
    assert.equal(await page.evaluate(() => current().term.options.theme.background), '#00000000');
    assert.equal(await page.evaluate(() => current().term.options.theme.foreground), theme === 'light' ? '#1a1a1a' : '#e6e6e6');
  }
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').showModal());
  await importPreset('Binary', Buffer.from(binary, 'base64'));
  await importPreset('Translucent', Buffer.from(xml({ ...base, 'Background Color': color(.1, .2, .3, 'sRGB', .5) })));
  await page.evaluate(() => document.querySelector('#ansi-schemes-dialog').close());
  assert.equal(await page.evaluate(() => current().term.options.theme.background), '#00000000');
  assert.equal(await page.evaluate(() => getComputedStyle(current().host.querySelector('.xterm-scrollable-element')).backgroundColor), 'rgba(0, 0, 0, 0)');
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#terminal-output')).backgroundColor), 'rgba(0, 0, 0, 0)');
  await page.evaluate(async () => chooseAppearance({ ansiPalette: 'soft' }));
  assert.equal(await page.evaluate(() => document.documentElement.style.getPropertyValue('--terminal-bold')), '');
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#terminal-output')).backgroundColor), 'rgba(0, 0, 0, 0)');
  await page.evaluate(async () => chooseAppearance({ ansiPalette: 'custom', ansiScheme: 'Dual' }));
  await page.reload({ waitUntil: 'networkidle' });
  await page.waitForFunction(() => appearance.ansiScheme === 'Dual');
  assert.equal(await page.evaluate(() => terminalTheme().background), '#00000000');
  assert.equal(await page.evaluate(() => appearance.ansiSchemes.find(s => s.name === 'Dual').lightColors.background), '#e6e6e6');
  assert.deepEqual(errors, []);
  fs.writeFileSync(path.join(stage, 'iterm-browser-results.json'), JSON.stringify({ passed: true, rendering, checks: ['XML/binary/UTF-16', 'sRGB/Calibrated/P3', 'all 29 color slots', 'malformed multi-file import rollback', 'scheme background ignored and switch removed', 'ANSI/truecolor/inverse/selection preserved', 'bold/underline/link', 'OSC background update/reset across tabs', 'light/dark live switch', 'preset reset', 'saved mode colors after reload'] }, null, 2));
  console.log('PASS: iTerm2 parser, import/API persistence, background, renderer, mode switching and reload.');
} finally { await browser.close(); }
