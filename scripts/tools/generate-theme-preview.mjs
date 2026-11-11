// 全局主题预览：复用产品界面的真实标记与样式，只补充本地演示数据。
// 布局改动留在产品里；本文件不访问真实服务器，也不保存任何产品偏好。
import fs from 'node:fs';
const root = new URL('../../', import.meta.url);
const source = fs.readFileSync(new URL('web/index.html', root), 'utf8');
const styles = source.match(/<link rel="stylesheet"[^>]+>/g).join('\n  ').replaceAll('href="', 'href="../../web/');
const workspace = source.slice(source.indexOf('  <svg class="icon-sprite"'), source.indexOf('  <div id="monitor-backdrop"')).replaceAll('src="assets/', 'src="../../web/assets/');
const managerStart = source.indexOf('  <dialog id="ui-styles-dialog"');
const manager = source.slice(managerStart, source.indexOf('</dialog>', managerStart) + 9);
const settings = source.slice(source.indexOf('  <div class="settings-menu"'), source.indexOf('  <dialog id="about-dialog"'));
const behavior = String.raw`
(() => {
  'use strict';
  const root = document.documentElement, params = new URLSearchParams(location.search);
  const system = matchMedia('(prefers-color-scheme: dark)');
  const $ = selector => document.querySelector(selector);
  // 这些设置行由产品脚本在运行时补齐；预览里镜像同样的控件。
  const settingsAnchor = $('#settings-main-view .scale-setting');
  function menuButton(id, label, icon, hint = '') {
    const button = document.createElement('button'); button.id = id; button.type = 'button'; button.role = 'menuitem';
    button.innerHTML = '<svg><use href="#i-' + icon + '"/></svg>' + label + (hint ? '<span>' + hint + '</span>' : '');
    return button;
  }
  settingsAnchor.before(menuButton('connection-config-document', '连接配置文档', 'folder', '升级与备份'));
  for (const [id, title, options, value] of [
    ['concise-mode-setting', '简洁模式', [['true','开启'],['false','关闭']], 'false'],
    ['monitor-visible', '监控面板', [['true','显示'],['false','隐藏']], 'true'],
    ['files-position', 'SFTP面板', [['bottom','终端下方'],['right','终端右侧'],['top','终端上方']], 'right'],
    ['follow-terminal-setting', '跟随终端目录', [['true','开启'],['false','关闭']], 'true'],
  ]) {
    const row = document.createElement('div'); row.className = 'scale-setting placement-setting';
    row.innerHTML = '<label for="' + id + '">' + title + '</label><select id="' + id + '" aria-label="' + title + '">' + options.map(([key,text]) => '<option value="' + key + '">' + text + '</option>').join('') + '</select>';
    row.querySelector('select').value = value; settingsAnchor.before(row);
  }
  $('#settings-main-view').append(menuButton('check-updates', '检查更新', 'refresh', '手动'), menuButton('update-mirror-settings', '更新加速域名', 'network', 'GitHub 加速'));
  $('#reset-layout').before(menuButton('manage-prompt-colors', '提示符颜色', 'terminal'));
  const systemRow = document.createElement('label'); systemRow.className = 'system-theme-setting'; systemRow.htmlFor = 'follow-system-theme';
  systemRow.innerHTML = '<span>明暗跟随系统<small id="follow-system-theme-hint"></small></span><input id="follow-system-theme" type="checkbox" role="switch" aria-describedby="follow-system-theme-hint">';
  $('#settings-appearance-view .settings-submenu-heading').after(systemRow);
  $('#ui-scale').value = '1';
  const styleNames = { classic: '经典', frosted: '液态玻璃' };
  const validStyle = value => ['classic', 'frosted'].includes(value) ? value : 'frosted';
  let style = validStyle(params.get('style'));
  let preference = ['light','dark','system'].includes(params.get('mode')) ? params.get('mode') : 'dark';
  function reflect() {
    root.dataset.theme = root.dataset.mode = preference === 'system' ? (system.matches ? 'dark' : 'light') : preference;
    root.dataset.themePreference = preference;
    root.style.colorScheme = root.dataset.theme;
    root.dataset.uiStyle = style;
    $('#follow-system-theme').checked = preference === 'system';
    $('#follow-system-theme-hint').textContent = preference === 'system' ? '当前' + (root.dataset.theme === 'dark' ? '深色' : '浅色') + ' · 手动切换明暗后关闭跟随' : '开启后随系统自动切换';
    const dark = root.dataset.theme === 'dark';
    $('#theme-toggle').setAttribute('aria-pressed', String(dark));
    $('#theme-toggle').setAttribute('aria-label', dark ? '切换到浅色模式' : '切换到深色模式');
    for (const button of document.querySelectorAll('[data-theme-mode]')) button.setAttribute('aria-pressed', String(button.dataset.themeMode === preference));
    for (const button of document.querySelectorAll('#ui-styles-dialog [data-ui-style]')) button.setAttribute('aria-pressed', String(button.dataset.uiStyle === style));
    $('#ui-style-status').textContent = '当前：' + styleNames[style] + ' · ' + (preference === 'system' ? '跟随系统' : dark ? '深色' : '浅色');
  }
  for (const button of document.querySelectorAll('[data-theme-mode]')) button.onclick = () => { preference = button.dataset.themeMode; reflect(); };
  for (const button of document.querySelectorAll('#ui-styles-dialog [data-ui-style]')) button.onclick = () => { style = validStyle(button.dataset.uiStyle); reflect(); };
  $('#theme-toggle').onclick = () => { preference = root.dataset.theme === 'dark' ? 'light' : 'dark'; reflect(); };
  const menu = $('#settings-menu');
  function positionSettings() {
    if (menu.hidden) return;
    const rect = $('#settings-button').getBoundingClientRect();
    menu.style.left = Math.max(8, Math.min(rect.left, innerWidth - menu.offsetWidth - 8)) + 'px';
    menu.style.top = rect.bottom + 6 + 'px';
  }
  function showSettingsView(view) {
    for (const name of ['main','appearance','other']) $('#settings-' + name + '-view').hidden = name !== view;
    $('#manage-appearance-config').setAttribute('aria-expanded', String(view === 'appearance'));
    $('#manage-other-features').setAttribute('aria-expanded', String(view === 'other'));
    positionSettings();
    (view === 'appearance' ? $('#manage-ui-styles') : view === 'other' ? $('#feature-auto-commands') : $('#manage-appearance-config')).focus();
  }
  function setSettings(open) {
    menu.hidden = !open; $('#settings-button').setAttribute('aria-expanded', String(open));
    if (open) showSettingsView('main');
    else for (const id of ['manage-appearance-config','manage-other-features']) $('#' + id).setAttribute('aria-expanded', 'false');
  }
  $('#settings-button').onclick = () => setSettings(menu.hidden);
  $('#manage-ui-styles').onclick = () => { setSettings(false); $('#ui-styles-dialog').showModal(); };
  $('#follow-system-theme').onchange = event => { preference = event.target.checked ? 'system' : root.dataset.theme; reflect(); };
  menu.addEventListener('click', event => {
    const button = event.target.closest('button'); if (!button) return;
    if (button.dataset.settingsView) { showSettingsView(button.dataset.settingsView); return; }
    if (button.id === 'manage-ui-styles') return;
    setSettings(false);
    $('#preview-setting-title').textContent = button.childNodes[1]?.textContent || button.textContent;
    $('#preview-setting-dialog').showModal();
  });
  $('#close-preview-setting').onclick = () => $('#preview-setting-dialog').close();
  $('#preview-setting-dialog').addEventListener('close', () => $('#settings-button').focus());
  document.addEventListener('click', event => { if (!event.target.closest('#settings-menu,#settings-button')) setSettings(false); });
  document.addEventListener('keydown', event => {
    if (menu.hidden) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!$('#settings-main-view').hidden) { setSettings(false); $('#settings-button').focus(); } else showSettingsView('main');
    } else if (['ArrowUp','ArrowDown','Home','End'].includes(event.key) && event.target.tagName !== 'SELECT') {
      const controls = [...menu.querySelectorAll('input[type=checkbox],button')].filter(control => !control.disabled && control.getClientRects().length);
      const index = controls.indexOf(document.activeElement); event.preventDefault();
      controls[event.key === 'Home' ? 0 : event.key === 'End' ? controls.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + controls.length) % controls.length]?.focus();
    }
  });
  window.addEventListener('resize', positionSettings);
  $('#close-ui-styles').onclick = () => { $('#ui-styles-dialog').close(); $('#settings-button').focus(); };
  system.addEventListener('change', () => { if (preference === 'system') reflect(); });
  const values = {'system-os':'Ubuntu 24.04',uptime:'18 天 6 小时','system-load':'0.42 / 0.38 / 0.31','cpu-model':'AMD EPYC 7B13','cpu-cores':'4 核','server-ipv4':'192.0.2.24','server-ipv6':'2001:db8::24','disk-read':'12.4 MB/s','disk-write':'3.8 MB/s','monitor-state':'实时','process-count':'5'};
  for (const [id, value] of Object.entries(values)) $('#' + id).textContent = value;
  for (const [name, percent, detail] of [['cpu','18.4','4 核'],['memory','32.5','2.6 / 8 GB'],['swap','2.1','43 / 2048 MB']]) {
    const meter = $('#' + name + '-meter'); meter.querySelector('i').style.width = percent + '%'; meter.querySelector('b').textContent = percent + '%'; meter.querySelector('em').textContent = detail;
  }
  $('#disk-filesystems').hidden = false;
  $('#disk-filesystems').innerHTML = '<div class="metric disk-filesystem"><span title="/">/</span><div class="meter"><i style="width:43%"></i><b>43%</b><em>42.8 / 100 GB</em></div></div>';
  $('#network-interface').innerHTML = '<option>eth0</option>';
  $('.upload-color').textContent = '上行 86 KB/s'; $('.download-color').textContent = '下行 1.28 MB/s';
  $('#network-chart').setAttribute('viewBox', '0 0 270 65');
  $('#network-chart').innerHTML = '<g class="traffic-grid"><line x1="28" y1="6" x2="262" y2="6"/><line x1="28" y1="25" x2="262" y2="25"/><line x1="28" y1="44" x2="262" y2="44"/><text x="0" y="10">2M</text><text x="0" y="29">1M</text><text x="5" y="48">0</text><text class="traffic-time" x="28" y="61">60 秒前</text><text class="traffic-time" x="241" y="61">现在</text></g><g class="traffic-tx"><path class="traffic-line" d="M28 42 45 40 62 41 79 38 96 40 113 35 130 39 147 37 164 39 181 33 198 35 215 32 232 36 249 33 262 34"/></g><g class="traffic-rx"><path class="traffic-line" d="M28 36 45 30 62 34 79 22 96 27 113 18 130 25 147 13 164 21 181 10 198 17 215 8 232 20 249 12 262 17"/></g>';
  $('#ping-latency').innerHTML = '24 <small>ms</small>'; $('#ping-loss').textContent = '0%';
  $('#ping-samples').innerHTML = '<svg viewBox="0 0 270 54" preserveAspectRatio="none" role="img" aria-label="延迟示例曲线"><g class="latency-grid"><line x1="28" y1="6" x2="262" y2="6"/><line x1="28" y1="23" x2="262" y2="23"/><line x1="28" y1="40" x2="262" y2="40"/><text x="2" y="10">40</text><text x="2" y="27">20</text><text x="8" y="44">0</text></g><g class="latency-series"><path class="latency-line" d="M28 23 45 25 62 22 79 23 96 20 113 24 130 21 147 23 164 22 181 18 198 22 215 24 232 21 249 23 262 22"/></g><text class="latency-time" x="28" y="53">60 秒前</text><text class="latency-time" x="241" y="53">现在</text></svg>';
  $('#process-list').innerHTML = [['182 MB','8.2','node'],['96 MB','4.1','sshd'],['64 MB','2.6','nginx'],['38 MB','1.3','systemd'],['24 MB','0.8','bash']].map(([memory,cpu,name]) => '<tr><td>' + memory + '</td><td>' + cpu + '%</td><td>' + name + '</td></tr>').join('');
  const sessionNames = ['atlas-prod','build-runner'];
  function selectSession(name) {
    $('#terminal-meta-host').textContent = 'phoenix@' + name;
    for (const tab of document.querySelectorAll('[data-preview-session]')) { tab.classList.toggle('active', tab.dataset.previewSession === name); tab.setAttribute('aria-selected', String(tab.dataset.previewSession === name)); }
    $('#terminal-output').innerHTML = '<div class="terminal-session"><pre class="preview-terminal-copy">Last login: Thu Nov  5 09:18:42 2026\n\nphoenix@' + name + ' ~\n❯ cd /srv/workspace\n\nphoenix@' + name + ' /srv/workspace\n❯ ls\ndocs/     internal/     scripts/     web/\nREADME.md  go.mod        package.json\n\n❯ echo "Less noise. More focus."\nLess noise. More focus.\n\nphoenix@' + name + ' /srv/workspace ❯ ▋</pre></div>';
  }
  for (const name of sessionNames) {
    const tab = document.createElement('div'); tab.className = 'session-tab'; tab.dataset.previewSession = name; tab.role = 'tab'; tab.tabIndex = 0;
    tab.innerHTML = '<span class="status-dot green"></span><span class="session-tab-text"><span class="session-tab-label">' + name + '</span></span>';
    tab.onclick = () => selectSession(name); tab.onkeydown = event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectSession(name); } };
    $('#session-tabs').append(tab);
  }
  selectSession(sessionNames[0]);
  $('#terminal-status-dot').classList.add('green'); $('#terminal-current-badge').hidden = false;
  function showFiles(show) { $('#files-panel').hidden = $('#files-splitter').hidden = !show; $('#workspace').classList.toggle('files-hidden', !show); $('#toggle-sftp').setAttribute('aria-expanded', String(show)); }
  showFiles(true); $('#toggle-sftp').onclick = () => showFiles($('#files-panel').hidden); $('#close-files-panel').onclick = () => showFiles(false);
  $('#path-input').value = '/srv/workspace';
  const files = [['docs','目录','drwxr-xr-x'],['internal','目录','drwxr-xr-x'],['README.md','4.2 KB','-rw-r--r--'],['package.json','1.6 KB','-rw-r--r--']];
  function renderFiles() {
    const visible = files.filter(([name]) => name.includes($('#file-filter').value.trim()));
    $('#file-list').innerHTML = visible.map(([name,size,mode]) => '<tr tabindex="0"><td>' + name + '</td><td>' + size + '</td><td>11-05 09:18</td><td>' + mode + '</td><td>phoenix / staff</td></tr>').join('');
    $('#file-status-count').textContent = visible.length + ' 个项目'; $('#file-empty').hidden = visible.length > 0;
    for (const row of $('#file-list').children) row.onclick = () => { for (const item of $('#file-list').children) item.classList.toggle('selected', item === row); };
  }
  $('#file-filter').oninput = renderFiles; renderFiles();
  for (const button of document.querySelectorAll('[data-pane]')) button.onclick = () => {
    for (const pane of ['files','commands','common-apps','transfers']) $('#' + pane + '-view').hidden = pane !== button.dataset.pane;
    for (const tab of document.querySelectorAll('[data-pane]')) tab.classList.toggle('active', tab === button);
  };
  $('#command-form').onsubmit = event => event.preventDefault();
  $('#command-input').onkeydown = event => { if (event.key === 'Enter') event.preventDefault(); };
  function resize() { root.dataset.narrow = String(innerWidth < 1100); root.dataset.short = String(innerHeight < 800); }
  window.addEventListener('resize', resize); resize(); reflect();
  if (params.get('blur') === 'off') root.dataset.previewOpaque = 'true';
  if (params.get('panel') === 'themes') $('#ui-styles-dialog').showModal();
  if (params.get('panel') === 'settings-appearance') { setSettings(true); showSettingsView('appearance'); }
})();`;
const html = `<!doctype html>
<!-- Generated by scripts/tools/generate-theme-preview.mjs from the product UI.
     布局与经典样式与产品一致；预览数据为演示数据，不读写真实偏好。 -->
<html lang="zh-CN" id="global-themes-preview" data-design-preview="true" data-theme="dark" data-mode="dark" data-ui-style="frosted" data-files-position="right" data-monitor-visible="true">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>DevShell · 全局主题预览</title>
  ${styles}
  <style>
    /* 只有终端演示文本需要样式；全部控件使用产品 CSS。 */
    .preview-terminal-copy { margin:0; color:var(--terminal-foreground,#d3e0e8); font:14px/1.6 var(--terminal-family,monospace); white-space:pre; }
    :root[data-preview-opaque="true"][data-ui-style="frosted"] { --glass-surface:var(--glass-solid); --glass-toolbar:var(--glass-solid); --glass-sidebar:var(--glass-solid); --glass-popup:var(--glass-solid); --glass-terminal:var(--glass-terminal-solid); }
    [data-preview-opaque="true"] body::before { display:none; }
  </style>
</head>
<body>
${workspace}
${settings}
${manager}
<dialog id="preview-setting-dialog" aria-labelledby="preview-setting-title"><div class="dialog-heading"><h2 id="preview-setting-title"></h2><button type="button" id="close-preview-setting" class="icon-button" aria-label="关闭预览说明"><svg><use href="#i-close"/></svg></button></div><p>此设计稿展示主题效果与设置入口。该功能及已有配置保留在应用中，请在应用内使用。</p></dialog>
<script>${behavior}</script>
</body>
</html>
`;
fs.writeFileSync(new URL('docs/design/global-themes-preview.html', root), html);
console.log('全局主题预览已按产品标记与样式重新生成：docs/design/global-themes-preview.html');
