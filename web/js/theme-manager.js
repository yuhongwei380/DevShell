/* 主题管理（设置 → 外观配置 → 主题管理）：把当前整套外观（界面与终端字体
   颜色、ANSI 预设、终端字号、曲线样式、提示符颜色、明暗模式）
   保存为独立主题，应用时整套生效。主题列表整体存进 appearance.themes，
   只增删列表本身不会改变当前外观；应用走普通的字段级补丁与校验，引用的
   字体若已被删除会在应用时报错回滚，而不是留下半套外观。 */
'use strict';
window.DengThemeManager = (() => {
  const snapshotKeys = ['theme', 'uiStyle', 'uiFontId', 'uiTextColors', 'fontId', 'fontColors', 'fontBold', 'ansiPalette', 'terminalFontSize', 'chartStyles', 'promptUsernameColor', 'promptHostnameColor'];
  const mapKeys = new Set(['uiTextColors', 'fontColors', 'fontBold', 'chartStyles']);
  const ansiLabels = { soft: '柔和', vivid: '高饱和', bright: '鲜亮' };
  const themeLabels = { system: '跟随系统', light: '浅色', dark: '深色' };
  let dialog, list, nameInput, errorText, busy = false, confirmId = '';

  const themes = () => Array.isArray(appearance.themes) ? appearance.themes : [];
  function capture() {
    const snapshot = {};
    for (const key of snapshotKeys) {
      const value = appearance[key];
      snapshot[key] = mapKeys.has(key) ? structuredClone(value || {}) : value;
    }
    snapshot.theme = window.DevShellTheme?.preference || snapshot.theme || 'system';
    return snapshot;
  }
  function summary(theme) {
    const bits = [themeLabels[theme.theme || 'system'] || '跟随系统'];
    if (theme.terminalFontSize) bits.push(`终端字号 ${theme.terminalFontSize}`);
    bits.push(`ANSI ${ansiLabels[theme.ansiPalette] || '柔和'}`);
    if (Object.keys(theme.fontColors || {}).length) bits.push('自定义终端颜色');
    return bits.join(' · ');
  }
  function fail(message) { errorText.textContent = message || String(message || ''); }
  async function saveThemes(next) {
    await chooseAppearance({ themes: next });
    render();
  }
  async function run(action) {
    if (busy) return;
    busy = true; errorText.textContent = '';
    try { await action(); }
    catch (error) { fail(error?.message || String(error)); }
    finally { busy = false; render(); }
  }
  function applyTheme(theme) {
    return run(async () => {
      const patch = {};
      for (const key of snapshotKeys) {
        const value = theme[key];
        patch[key] = mapKeys.has(key) ? structuredClone(value || {}) : value;
        if (patch[key] === undefined || patch[key] === null) delete patch[key];
      }
      patch.theme = theme.theme || 'system';
      await chooseAppearance(patch);
      window.DevShellTheme?.set(patch.theme);
      toast(`已应用主题「${theme.name}」`);
    });
  }
  function validName(name, exceptID = '') {
    const trimmed = name.trim();
    if (!trimmed) return '请先输入主题名称。';
    if ([...trimmed].length > 48) return '主题名称最长 48 个字符。';
    if (themes().some(theme => theme.name === trimmed && theme.id !== exceptID)) return `已有同名主题「${trimmed}」。`;
    return '';
  }
  function createTheme() {
    const problem = validName(nameInput.value);
    if (problem) { fail(problem); return; }
    const name = nameInput.value.trim();
    const theme = { id: crypto.randomUUID(), name, ...capture() };
    return run(async () => {
      await saveThemes([...themes(), theme]);
      nameInput.value = '';
      toast(`已把当前外观保存为主题「${name}」`);
    });
  }
  function duplicateTheme(theme) {
    const base = `${theme.name} 副本`;
    let name = base, index = 2;
    while (themes().some(item => item.name === name)) name = `${base} ${index++}`;
    return run(async () => {
      await saveThemes([...themes(), { ...structuredClone(theme), id: crypto.randomUUID(), name }]);
      toast(`已复制为「${name}」`);
    });
  }
  function renameTheme(theme, value) {
    const problem = validName(value, theme.id);
    if (problem) { fail(problem); return Promise.resolve(); }
    const name = value.trim();
    return run(async () => {
      await saveThemes(themes().map(item => item.id === theme.id ? { ...item, name } : item));
      toast('主题名称已更新');
    });
  }
  function removeTheme(theme) {
    // 两步确认：第一次点“删除”只把它切换为“确认删除”，几秒后自动还原。
    if (confirmId !== theme.id) { confirmId = theme.id; render(); setTimeout(() => { if (confirmId === theme.id) { confirmId = ''; render(); } }, 4000); return Promise.resolve(); }
    confirmId = '';
    return run(async () => {
      await saveThemes(themes().filter(item => item.id !== theme.id));
      toast(`已删除主题「${theme.name}」`);
    });
  }
  function row(theme) {
    const row = document.createElement('div'); row.className = 'theme-row'; row.dataset.id = theme.id;
    const info = document.createElement('div'); info.className = 'theme-info';
    const title = document.createElement('strong'); title.textContent = theme.name;
    const note = document.createElement('span'); note.className = 'theme-summary'; note.textContent = summary(theme);
    info.append(title, note);
    const actions = document.createElement('div'); actions.className = 'theme-actions';
    const button = (action, label, primary = false) => {
      const control = document.createElement('button'); control.type = 'button'; control.dataset.act = action; control.textContent = label;
      if (primary) control.classList.add('primary-button');
      if (action === 'delete') control.classList.add('danger-button');
      control.disabled = busy;
      return control;
    };
    const apply = button('apply', '应用', true);
    apply.onclick = () => applyTheme(theme);
    actions.append(apply, button('rename', '重命名'), button('duplicate', '复制'), button('delete', confirmId === theme.id ? '确认删除？' : '删除'));
    actions.querySelector('[data-act=rename]').onclick = () => {
      const editor = document.createElement('div'); editor.className = 'theme-rename';
      const input = document.createElement('input'); input.value = theme.name; input.maxLength = 48; input.setAttribute('aria-label', '主题名称');
      const save = document.createElement('button'); save.type = 'button'; save.textContent = '保存'; save.className = 'primary-button';
      const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = '取消';
      editor.append(input, save, cancel);
      info.replaceChildren(editor, note);
      input.focus(); input.select();
      const commit = () => renameTheme(theme, input.value);
      save.onclick = commit;
      input.onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); commit(); } if (event.key === 'Escape') { event.preventDefault(); render(); } };
      cancel.onclick = () => render();
    };
    actions.querySelector('[data-act=duplicate]').onclick = () => duplicateTheme(theme);
    actions.querySelector('[data-act=delete]').onclick = () => removeTheme(theme);
    row.append(info, actions);
    return row;
  }
  function render() {
    const items = themes();
    list.replaceChildren(...items.map(theme => row(theme)));
    const empty = document.createElement('p'); empty.className = 'theme-empty';
    empty.textContent = items.length ? '' : '还没有主题：调好外观后输入名称，点“从当前外观新建”即可保存第一套。';
    empty.hidden = items.length > 0;
    list.append(empty);
  }
  function initialize() {
    dialog = document.createElement('dialog'); dialog.id = 'theme-manager-dialog';
    dialog.innerHTML = `<form id="theme-manager-form" novalidate><div class="dialog-heading"><h2 id="theme-manager-title">主题管理</h2><button type="button" class="icon-button" id="close-theme-manager" aria-label="关闭主题管理"><svg><use href="#i-close"/></svg></button></div><p>把当前整套外观（明暗模式、界面与终端字体颜色、ANSI 预设、终端字号、曲线样式、提示符颜色）保存为独立主题；应用时整套生效，字体缺失会报错并保持原样。</p><div class="theme-create"><input id="theme-new-name" maxlength="48" placeholder="新主题名称" aria-label="新主题名称"><button type="button" id="theme-create-button" class="primary-button"><svg><use href="#i-plus"/></svg>从当前外观新建</button></div><div class="theme-list" id="theme-list"></div><p id="theme-manager-error" role="alert"></p></form>`;
    document.body.append(dialog);
    const form = dialog.querySelector('form');
    list = form.querySelector('#theme-list');
    nameInput = form.querySelector('#theme-new-name');
    errorText = form.querySelector('#theme-manager-error');
    form.querySelector('#theme-create-button').onclick = () => createTheme();
    nameInput.onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); createTheme(); } };
    const close = () => { if (!busy) dialog.close(); };
    form.querySelector('#close-theme-manager').onclick = close;
    dialog.oncancel = event => { event.preventDefault(); close(); };
    form.onsubmit = event => event.preventDefault();
    const opener = document.querySelector('#manage-themes');
    if (opener) opener.onclick = safe(() => {
      setSettingsMenu(false);
      confirmId = ''; errorText.textContent = ''; render();
      if (!dialog.open) dialog.showModal();
    });
  }
  document.addEventListener('DOMContentLoaded', initialize, { once: true });
  return { themes, capture };
})();
