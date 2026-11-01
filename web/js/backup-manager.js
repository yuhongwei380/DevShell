/* Session backup export/import: connections, groups, managed keys, proxies and known host keys move between installations. */
'use strict';
(function () {
  const picker = document.getElementById('backup-picker');
  const dialog = document.getElementById('backup-dialog');
  const secrets = document.getElementById('backup-secrets');
  const status = document.getElementById('backup-status');
  if (!picker || !dialog) return;
  document.getElementById('export-backup').addEventListener('click', () => {
    if (window.DengSecurityLock?.isLocked()) return;
    status.textContent = '';
    dialog.showModal();
  });
  document.getElementById('close-backup').addEventListener('click', () => dialog.close());
  document.getElementById('do-export-backup').addEventListener('click', async () => {
    const button = document.getElementById('do-export-backup');
    button.disabled = true;
    try {
      const backup = await api(`/api/backup/export?secrets=${secrets.checked ? '1' : '0'}`);
      const now = new Date(), pad = value => String(value).padStart(2, '0');
      const name = `devshell-backup-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}.json`;
      const link = document.createElement('a');
      link.href = URL.createObjectURL(new Blob([JSON.stringify(backup, null, 2)], { type: 'application/json' }));
      link.download = name;
      link.click();
      setTimeout(() => URL.revokeObjectURL(link.href), 30000);
      status.textContent = `已导出 ${name}`;
      toast(`已导出备份：${(backup.connections || []).length} 个连接${secrets.checked ? '（含密码与私钥，请妥善保管文件）' : ''}`);
      dialog.close();
    } catch (error) {
      status.textContent = `导出失败：${error?.message || error}`;
    } finally {
      button.disabled = false;
    }
  });
  document.getElementById('import-backup').addEventListener('click', () => {
    if (window.DengSecurityLock?.isLocked()) return;
    picker.value = '';
    picker.click();
  });
  picker.addEventListener('change', async () => {
    const file = picker.files?.[0];
    if (!file) return;
    try {
      if (file.size > 32 * 1024 * 1024) throw new Error('备份文件超过 32 MiB');
      const backup = JSON.parse(await file.text());
      const summary = await post('/api/backup/import', backup);
      if (typeof refreshServerManagerHistory === 'function') await refreshServerManagerHistory();
      toast(`导入完成：新增 ${summary.connectionsAdded} 个连接、${summary.keysAdded} 个密钥，跳过 ${summary.connectionsSkipped} 个重复`);
    } catch (error) {
      toast(`导入失败：${error?.message || error}`);
    }
  });
})();
