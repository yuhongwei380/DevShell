// Built-in workspace themes share the existing portable appearance store.
(() => {
  'use strict';
  const dialog = document.getElementById('theme-manager-dialog');
  const status = document.getElementById('theme-manager-status');
  const styles = [...dialog.querySelectorAll('[data-theme-style]')];
  const modes = [...dialog.querySelectorAll('[data-theme-mode]')];
  let saving = false;
  function reflect() {
    const style = appearance.uiStyle === 'frosted' ? 'frosted' : 'classic';
    for (const button of styles) button.setAttribute('aria-pressed', String(button.dataset.themeStyle === style));
    for (const button of modes) {
      button.disabled = saving;
      button.setAttribute('aria-pressed', String(button.dataset.themeMode === window.DevShellTheme.preference));
    }
    if (!saving) status.textContent = `当前：${style === 'frosted' ? '毛玻璃' : '经典'} · ${window.DevShellTheme.preference === 'system' ? '跟随系统' : window.DevShellTheme.current === 'dark' ? '深色' : '浅色'}`;
  }
  async function select(patch) {
    if (saving) return;
    saving = true;
    dialog.setAttribute('aria-busy', 'true');
    for (const button of [...styles, ...modes]) button.disabled = true;
    status.textContent = '正在应用…';
    try {
      await chooseAppearance(patch);
      if (patch.theme) window.DevShellTheme.set(patch.theme);
    } catch (error) {
      toast(`主题未能保存：${error.message || error}`);
    } finally {
      saving = false;
      dialog.removeAttribute('aria-busy');
      for (const button of [...styles, ...modes]) button.disabled = false;
      reflect();
    }
  }
  function open() {
    setSettingsMenu(false);
    reflect();
    if (!dialog.open) dialog.showModal();
    styles.find(button => button.getAttribute('aria-pressed') === 'true')?.focus();
  }
  for (const button of styles) button.onclick = () => select({ uiStyle: button.dataset.themeStyle });
  for (const button of modes) button.onclick = () => select({ theme: button.dataset.themeMode });
  document.getElementById('manage-themes').onclick = open;
  document.getElementById('close-theme-manager').onclick = () => dialog.close();
  dialog.addEventListener('close', () => document.getElementById('settings-button').focus());
  window.addEventListener('cloudshell:theme', reflect);
  window.DengThemeManager = Object.freeze({ open, reflect });
  reflect();
})();
