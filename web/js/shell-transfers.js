'use strict';
// Keep transfer feedback beside the shell even while the SFTP pane is closed.
window.DevShellTransfers = (() => {
  function update(tasks, viewing) {
    const button = document.querySelector('#shell-transfer-status');
    if (!button) return;
    const results = tasks.filter(task => ['done', 'failed'].includes(task.status));
    const fresh = results.filter(task => !task.historicalResult && !task.resultSeen && !task.completionNotified);
    if (fresh.length && !document.hidden && !window.DevShellWindowHidden && !window.DengSecurityLock?.isLocked()) {
      const failed = fresh.filter(task => task.status === 'failed').length;
      toast(failed ? `上传失败 ${failed} 项，请查看传输任务` : `上传已完成${fresh.length > 1 ? `（${fresh.length} 项）` : ''}`);
      for (const task of fresh) task.completionNotified = true;
    }
    const active = tasks.filter(task => ['queued', 'uploading'].includes(task.status));
    const unread = results.filter(task => !task.resultSeen);
    button.hidden = viewing || (!active.length && !unread.length);
    if (button.hidden) return;
    if (active.length) {
      const total = active.reduce((sum, task) => sum + (Number(task.total) || 0), 0);
      const done = active.reduce((sum, task) => sum + (Number(task.done) || 0), 0);
      // Progress is remote-acknowledged bytes; a 100% byte count is not success.
      const percent = total ? ` · ${Math.min(100, Math.floor(done / total * 100))}%` : '';
      button.textContent = `${active.some(task => task.status === 'uploading') ? '上传中' : '等待上传'}${percent} · ${active.length} 项 · 查看传输任务`;
    } else {
      const failed = unread.filter(task => task.status === 'failed').length;
      button.textContent = failed ? `上传失败 ${failed} 项 · 查看传输任务` : `上传已完成 · ${unread.length} 项 · 查看传输任务`;
    }
    button.onclick = () => showPane('transfers');
  }
  return { update };
})();
