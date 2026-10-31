'use strict';
// Keep transfer feedback inside the shell even while the SFTP pane is closed.
window.DevShellTransfers = (() => {
  function update(tasks, viewing, sessionID = '') {
    const button = document.querySelector('#shell-transfer-status');
    if (!button) return;
    // A transfer list is shared by all tabs and detached windows. Keep the
    // shell indicator tied to the terminal that initiated the upload; older
    // local records without a session id remain visible for compatibility.
    const scoped = sessionID ? tasks.filter(task => !task.sessionId || task.sessionId === sessionID) : tasks;
    const results = scoped.filter(task => ['done', 'failed'].includes(task.status));
    const fresh = results.filter(task => !task.historicalResult && !task.resultSeen && !task.completionNotified);
    if (fresh.length && !document.hidden && !window.DevShellWindowHidden && !window.DengSecurityLock?.isLocked()) {
      const failed = fresh.filter(task => task.status === 'failed').length;
      toast(failed ? `上传失败 ${failed} 项，请查看传输任务` : `上传已完成${fresh.length > 1 ? `（${fresh.length} 项）` : ''}`);
      for (const task of fresh) task.completionNotified = true;
    }
    const active = scoped.filter(task => ['queued', 'uploading'].includes(task.status));
    const unread = results.filter(task => !task.resultSeen);
    // Keep active progress in the terminal even when the SFTP pane is open.
    // Completed results can stay in the transfer pane once it is visible.
    button.hidden = !active.length && (viewing || !unread.length);
    if (button.hidden) {
      button.style?.removeProperty?.('--transfer-progress');
      button.removeAttribute?.('aria-valuenow');
      button.removeAttribute?.('aria-busy');
      return;
    }
    if (active.length) {
      const total = active.reduce((sum, task) => sum + (Number(task.total) || 0), 0);
      const done = active.reduce((sum, task) => sum + (Number(task.done) || 0), 0);
      // Progress is remote-acknowledged bytes; a 100% byte count is not success.
      const value = total ? Math.min(100, Math.floor(done / total * 100)) : 0;
      button.title = `${active.some(task => task.status === 'uploading') ? '上传中' : '等待上传'} · ${value}%`;
      button.setAttribute?.('aria-valuemin', '0');
      button.setAttribute?.('aria-valuemax', '100');
      button.setAttribute?.('aria-valuenow', String(value));
      button.setAttribute?.('aria-busy', 'true');
      // Reuse the transfer-list ring: grey track, green arc, percent number inside the ring.
      button.innerHTML = `<span class="transfer-progress mini" aria-hidden="true">` +
        `<svg viewBox="0 0 48 48"><circle class="transfer-ring-track" cx="24" cy="24" r="20" pathLength="100"></circle>` +
        `<circle class="transfer-ring-fill" cx="24" cy="24" r="20" pathLength="100" stroke-dasharray="${value} 100"></circle></svg>` +
        `<span class="transfer-percent">${value}</span></span>` +
        `<span class="shell-transfer-label">${active.length} 项 · 查看传输任务</span>`;
    } else {
      button.removeAttribute?.('aria-valuenow');
      button.removeAttribute?.('aria-busy');
      const failed = unread.filter(task => task.status === 'failed').length;
      button.textContent = failed ? `上传失败 ${failed} 项 · 查看传输任务` : `上传已完成 · ${unread.length} 项 · 查看传输任务`;
    }
    button.onclick = () => showPane('transfers');
  }
  return { update };
})();
