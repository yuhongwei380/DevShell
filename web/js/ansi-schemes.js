/* iTerm2 配色方案管理：名称沿用文件名，同名覆盖，导入后立即应用。
   解析与颜色空间转换由 iterm-colors.js 完成；保留全部预设色和明暗变体。 */
'use strict';
window.DengAnsiSchemes = (() => {
  let dialog, list, countText, errorText, busy = false, confirmName = '';

  const schemes = () => Array.isArray(appearance.ansiSchemes) ? appearance.ansiSchemes : [];
  const activeName = () => appearance.ansiPalette === 'custom' ? (appearance.ansiScheme || '') : '';
  function fail(message) { errorText.textContent = message || String(message || ''); }
  async function run(action) {
    if (busy) return;
    busy = true; errorText.textContent = '';
    try { await action(); }
    catch (error) { const message = error?.message || String(error); fail(message); toast(`配色未能保存：${message}`); }
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
      const preset = window.DevShellItermColors.parse(await file.arrayBuffer());
      const name = schemeNameFor(file);
      const rest = next.filter(item => item.name !== name);
      if (rest.length + 1 > 64) throw new Error('配色方案最多 64 套，请先删除部分方案');
      next.length = 0; next.push(...rest, { name, ...preset });
      lastName = name;
    }
    await chooseAppearance({ ansiSchemes: next, ansiPalette: 'custom', ansiScheme: lastName });
    toast(files.length === 1 ? `已导入 iTerm 配色「${lastName}」，已应用其 16 色 ANSI 与文件携带的前景、光标、选区等附加色` : `已导入 ${files.length} 套 iTerm 配色，已应用「${lastName}」`);
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
    const colors = colorsForAnsiScheme(scheme);
    const row = document.createElement('div'); row.className = 'scheme-row'; row.dataset.name = scheme.name;
    const info = document.createElement('div'); info.className = 'scheme-info';
    const title = document.createElement('strong'); title.textContent = scheme.name;
    const swatches = document.createElement('span'); swatches.className = 'scheme-swatches'; swatches.setAttribute('aria-hidden', 'true');
    for (const key of ansiColorKeys) {
      const chip = document.createElement('i');
      if (validFontColor(colors?.[key])) chip.style.background = colors[key];
      swatches.append(chip);
    }
    // 摘要只列出实际应用的颜色；文件背景色留作方案元数据。
    const schemeExtraNames = { foreground: '前景', cursor: '光标', cursorAccent: '光标文字', selectionBackground: '选区', selectionForeground: '选区文字', bold: '粗体', link: '链接', underline: '下划线' };
    const note = document.createElement('span'); note.className = 'scheme-summary';
    note.textContent = `${active ? '当前应用 · ' : ''}16 色 ANSI${scheme.lightColors || scheme.darkColors ? ' · 明暗配色' : ''}${Object.entries(schemeExtraNames).filter(([key]) => colors?.[key]).map(([, label]) => ` + ${label}`).join('')}`;
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
    dialog.innerHTML = `<form id="ansi-schemes-form" novalidate><div class="dialog-heading"><h2 id="ansi-schemes-title">配色方案</h2><button type="button" class="icon-button" id="close-ansi-schemes" aria-label="关闭配色方案"><svg><use href="#i-close"/></svg></button></div><p>支持 iTerm2 的 XML 和二进制 .itermcolors 文件，名称沿用文件名，同名导入即覆盖。按颜色空间还原 ANSI、前景、粗体、光标、选区、链接与下划线色，明暗配色随界面切换；终端保持界面原有底色。文件中的背景及其余装饰色一并保存在方案中。</p><div class="scheme-toolbar"><button type="button" id="scheme-import-button" class="primary-button"><svg><use href="#i-plus"/></svg>导入 iTerm 配色…</button><span id="scheme-count"></span></div><div class="scheme-list" id="scheme-list"></div><p id="scheme-error" role="alert"></p></form>`;
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
