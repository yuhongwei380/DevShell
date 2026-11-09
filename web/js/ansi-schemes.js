/* 配色方案管理（设置 → 外观配置 → 配色方案）：支持导入多套 iTerm Color
   Scheme（.itermcolors），方案名称沿用导入文件名，同名导入即覆盖；可在本页
   应用、重命名与两步确认删除。列表整体存进 appearance.ansiSchemes，当前应用的
   方案名存进 appearance.ansiScheme（ansiPalette 为 custom）。解析在客户端完成，
   只取「Ansi 0..15 Color」16 键换算为 #RRGGBB；背景、前景、光标等非 ANSI 键
   忽略——终端背景保持透明、前景仍随字体颜色。悬空的方案引用由后端回落到柔和。 */
'use strict';
window.DengAnsiSchemes = (() => {
  // iTerm Color Schemes（.itermcolors）是 XML plist 的最小解析。
  const plistTag = node => node.tagName.toLowerCase();
  function plistDict(dict) { const result = {}; let key = ''; for (const child of dict.children) { if (plistTag(child) === 'key') { key = child.textContent; continue; } if (key) { result[key] = plistValue(child); key = ''; } } return result; }
  function plistValue(node) { const tag = plistTag(node); if (tag === 'dict') return plistDict(node); if (tag === 'real' || tag === 'integer') return Number(node.textContent); if (tag === 'string') return node.textContent; if (tag === 'true') return true; if (tag === 'false') return false; return null; }
  function itermColorToHex(entry, index) {
    const channel = name => { const value = Number(entry?.[name]); if (!Number.isFinite(value)) throw new Error(`iTerm 配色缺少「Ansi ${index} Color」的 ${name}`); return Math.round(Math.min(1, Math.max(0, value)) * 255); };
    const hex = value => value.toString(16).padStart(2, '0');
    return `#${hex(channel('Red Component'))}${hex(channel('Green Component'))}${hex(channel('Blue Component'))}`;
  }
  function parseItermAnsiColors(text) {
    const parsed = new DOMParser().parseFromString(text, 'application/xml');
    if (parsed.querySelector('parsererror')) throw new Error('无法解析 iTerm 配色文件');
    const plist = parsed.documentElement, dict = plist?.firstElementChild;
    if (!plist || plistTag(plist) !== 'plist' || !dict || plistTag(dict) !== 'dict') throw new Error('不是有效的 iTerm 配色文件');
    const top = plistDict(dict), colors = {};
    for (let index = 0; index < ansiColorKeys.length; index++) {
      const entry = top[`Ansi ${index} Color`];
      if (!entry || typeof entry !== 'object') throw new Error(`iTerm 配色缺少「Ansi ${index} Color」`);
      colors[ansiColorKeys[index]] = itermColorToHex(entry, index);
    }
    return colors;
  }

  let dialog, list, countText, errorText, busy = false, confirmName = '';

  const schemes = () => Array.isArray(appearance.ansiSchemes) ? appearance.ansiSchemes : [];
  const activeName = () => appearance.ansiPalette === 'custom' ? (appearance.ansiScheme || '') : '';
  function fail(message) { errorText.textContent = message || String(message || ''); }
  async function run(action) {
    if (busy) return;
    busy = true; errorText.textContent = '';
    try { await action(); }
    catch (error) { fail(error?.message || String(error)); }
    finally { busy = false; render(); }
  }
  // 设置页的 ANSI 预设下拉：静态项为三套预设，导入的方案挂 optgroup，值前缀 scheme:。
  function syncOptions() {
    const select = document.querySelector('#ansi-palette'); if (!select) return;
    select.querySelector('optgroup[data-ansi-schemes]')?.remove();
    const items = schemes(); if (!items.length) return;
    const group = document.createElement('optgroup'); group.label = '导入的配色方案'; group.dataset.ansiSchemes = '';
    for (const scheme of items) {
      const option = document.createElement('option');
      option.value = `scheme:${scheme.name}`; option.textContent = scheme.name;
      group.append(option);
    }
    select.append(group);
    window.DengSelect?.refresh(select);
  }
  // 名称沿用导入文件名（去掉 .itermcolors 后缀），超长截断；同名导入即覆盖。
  function schemeNameFor(file) {
    let base = file.name.replace(/\.itermcolors$/i, '').trim() || '未命名方案';
    if ([...base].length > 48) base = [...base].slice(0, 48).join('');
    return base;
  }
  async function importFiles(files) {
    const next = [...schemes()];
    let lastName = '';
    for (const file of files) {
      if (file.size > 256 * 1024) throw new Error(`「${file.name}」太大，请选择更小的 .itermcolors 文件`);
      const colors = parseItermAnsiColors(await file.text());
      const name = schemeNameFor(file);
      const rest = next.filter(item => item.name !== name);
      if (rest.length + 1 > 64) throw new Error('配色方案最多 64 套，请先删除部分方案');
      next.length = 0; next.push(...rest, { name, colors });
      lastName = name;
    }
    await chooseAppearance({ ansiSchemes: next, ansiPalette: 'custom', ansiScheme: lastName });
    toast(files.length === 1 ? `已导入 iTerm 配色「${lastName}」，已应用其 16 色 ANSI` : `已导入 ${files.length} 套 iTerm 配色，已应用「${lastName}」`);
  }
  function applyScheme(scheme) {
    return run(async () => {
      await chooseAppearance({ ansiPalette: 'custom', ansiScheme: scheme.name });
      toast(`已应用配色方案「${scheme.name}」`);
    });
  }
  function validName(name, exceptName = '') {
    const trimmed = name.trim();
    if (!trimmed) return '请先输入配色方案名称。';
    if ([...trimmed].length > 48) return '配色方案名称最长 48 个字符。';
    if (schemes().some(item => item.name === trimmed && item.name !== exceptName)) return `已有同名方案「${trimmed}」。`;
    return '';
  }
  function renameScheme(scheme, value) {
    const problem = validName(value, scheme.name);
    if (problem) { fail(problem); return Promise.resolve(); }
    const name = value.trim();
    const patch = { ansiSchemes: schemes().map(item => item.name === scheme.name ? { ...item, name } : item) };
    if (activeName() === scheme.name) patch.ansiScheme = name;
    return run(async () => {
      await chooseAppearance(patch);
      toast('配色方案名称已更新');
    });
  }
  function removeScheme(scheme) {
    // 两步确认：第一次点“删除”只把它切换为“确认删除”，几秒后自动还原。
    if (confirmName !== scheme.name) { confirmName = scheme.name; render(); setTimeout(() => { if (confirmName === scheme.name) { confirmName = ''; render(); } }, 4000); return Promise.resolve(); }
    confirmName = '';
    const patch = { ansiSchemes: schemes().filter(item => item.name !== scheme.name) };
    if (activeName() === scheme.name) patch.ansiPalette = 'soft';
    return run(async () => {
      await chooseAppearance(patch);
      toast(`已删除配色方案「${scheme.name}」`);
    });
  }
  function row(scheme) {
    const active = scheme.name === activeName();
    const row = document.createElement('div'); row.className = 'scheme-row'; row.dataset.name = scheme.name;
    const info = document.createElement('div'); info.className = 'scheme-info';
    const title = document.createElement('strong'); title.textContent = scheme.name;
    const swatches = document.createElement('span'); swatches.className = 'scheme-swatches'; swatches.setAttribute('aria-hidden', 'true');
    for (const key of ansiColorKeys) {
      const chip = document.createElement('i');
      if (validFontColor(scheme.colors?.[key])) chip.style.background = scheme.colors[key];
      swatches.append(chip);
    }
    const note = document.createElement('span'); note.className = 'scheme-summary'; note.textContent = active ? '当前应用 · 16 色 ANSI' : '16 色 ANSI';
    info.append(title, swatches, note);
    const actions = document.createElement('div'); actions.className = 'scheme-actions';
    const button = (action, label, primary = false) => {
      const control = document.createElement('button'); control.type = 'button'; control.dataset.act = action; control.textContent = label;
      if (primary) control.classList.add('primary-button');
      if (action === 'delete') control.classList.add('danger-button');
      control.disabled = busy;
      return control;
    };
    const apply = button('apply', active ? '已应用' : '应用', true);
    apply.disabled = active || busy;
    apply.onclick = () => applyScheme(scheme);
    actions.append(apply, button('rename', '重命名'), button('delete', confirmName === scheme.name ? '确认删除？' : '删除'));
    actions.querySelector('[data-act=rename]').onclick = () => {
      const editor = document.createElement('div'); editor.className = 'scheme-rename';
      const input = document.createElement('input'); input.value = scheme.name; input.maxLength = 48; input.setAttribute('aria-label', '配色方案名称');
      const save = document.createElement('button'); save.type = 'button'; save.textContent = '保存'; save.className = 'primary-button';
      const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = '取消';
      editor.append(input, save, cancel);
      info.replaceChildren(editor, swatches, note);
      input.focus(); input.select();
      const commit = () => renameScheme(scheme, input.value);
      save.onclick = commit;
      input.onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); commit(); } if (event.key === 'Escape') { event.preventDefault(); render(); } };
      cancel.onclick = () => render();
    };
    actions.querySelector('[data-act=delete]').onclick = () => removeScheme(scheme);
    row.append(info, actions);
    return row;
  }
  function render() {
    const items = schemes();
    list.replaceChildren(...items.map(scheme => row(scheme)));
    const empty = document.createElement('p'); empty.className = 'scheme-empty';
    empty.textContent = '还没有配色方案：点「导入 iTerm 配色…」选择 .itermcolors 文件，方案名称将沿用文件名。';
    empty.hidden = items.length > 0;
    list.append(empty);
    if (countText) countText.textContent = items.length ? `已导入 ${items.length} 套，最多 64 套` : '';
  }
  function initialize() {
    dialog = document.createElement('dialog'); dialog.id = 'ansi-schemes-dialog';
    dialog.innerHTML = `<form id="ansi-schemes-form" novalidate><div class="dialog-heading"><h2 id="ansi-schemes-title">配色方案</h2><button type="button" class="icon-button" id="close-ansi-schemes" aria-label="关闭配色方案"><svg><use href="#i-close"/></svg></button></div><p>支持导入多套 .itermcolors 配色方案，名称沿用导入文件名，同名导入即覆盖；仅应用其 16 色 ANSI，背景、前景、光标等其他键会被忽略。</p><div class="scheme-toolbar"><button type="button" id="scheme-import-button" class="primary-button"><svg><use href="#i-plus"/></svg>导入 iTerm 配色…</button><span id="scheme-count"></span></div><div class="scheme-list" id="scheme-list"></div><p id="scheme-error" role="alert"></p></form>`;
    document.body.append(dialog);
    const form = dialog.querySelector('form');
    list = form.querySelector('#scheme-list');
    countText = form.querySelector('#scheme-count');
    errorText = form.querySelector('#scheme-error');
    const picker = document.querySelector('#iterm-picker');
    form.querySelector('#scheme-import-button').onclick = () => { if (!busy) picker?.click(); };
    if (picker) picker.addEventListener('change', () => {
      const files = [...(picker.files || [])]; picker.value = '';
      if (files.length) run(() => importFiles(files));
    });
    const close = () => { if (!busy) dialog.close(); };
    form.querySelector('#close-ansi-schemes').onclick = close;
    dialog.oncancel = event => { event.preventDefault(); close(); };
    form.onsubmit = event => event.preventDefault();
    const opener = document.querySelector('#manage-ansi-schemes');
    if (opener) opener.onclick = safe(() => {
      setSettingsMenu(false);
      confirmName = ''; errorText.textContent = ''; render();
      if (!dialog.open) dialog.showModal();
    });
  }
  document.addEventListener('DOMContentLoaded', initialize, { once: true });
  return { schemes, syncOptions };
})();
