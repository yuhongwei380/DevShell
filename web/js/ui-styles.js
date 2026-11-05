/* 全局主题对话框：选择工作台样式（classic/frosted/polar/twilight）与明暗模式。
   样式只重绘整套界面材质，走普通 appearance 补丁持久化；与字体、ANSI、
   背景选择、布局互相独立，切换不重建终端也不中断连接。 */
(() => {
  'use strict';
  const dialog = document.getElementById('ui-styles-dialog');
  const status = document.getElementById('ui-style-status');
  const styles = [...dialog.querySelectorAll('[data-ui-style]')];
  const modes = [...dialog.querySelectorAll('[data-theme-mode]')];
  const names = { classic: '经典', frosted: '毛玻璃', polar: '极地', twilight: '夜幕' };
  let saving = false;
  function currentStyle() { return ['frosted', 'polar', 'twilight'].includes(appearance.uiStyle) ? appearance.uiStyle : 'classic'; }
  function reflect() {
    const style = currentStyle();
    for (const button of styles) button.setAttribute('aria-pressed', String(button.dataset.uiStyle === style));
    for (const button of modes) {
      button.disabled = saving;
      button.setAttribute('aria-pressed', String(button.dataset.themeMode === window.DevShellTheme.preference));
    }
    if (!saving) status.textContent = `当前：${names[style]} · ${window.DevShellTheme.preference === 'system' ? '跟随系统' : window.DevShellTheme.current === 'dark' ? '深色' : '浅色'}`;
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
  for (const button of styles) button.onclick = () => select({ uiStyle: button.dataset.uiStyle });
  for (const button of modes) button.onclick = () => select({ theme: button.dataset.themeMode });
  document.getElementById('manage-ui-styles').onclick = open;
  document.getElementById('close-ui-styles').onclick = () => dialog.close();
  dialog.addEventListener('close', () => document.getElementById('settings-button')?.focus());
  window.addEventListener('cloudshell:theme', reflect);
  window.DengUIStyles = Object.freeze({ open, reflect });
  reflect();
})();
