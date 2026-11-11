/* 本地 Shell 会话（仅支持的平台，Windows 已移除该功能）：「其他功能」里的本地
   终端开关打开后，“+”直接在本机开一个终端（macOS 登录 Shell 于家目录）。
   state.localShell 是各处分流标记；终端渲染、输入与断开处理复用与 SSH、串口
   一致的 WebSocket 中继协议。断开后的“重新连接”= 在本标签重新拉起一个本地会话。 */
'use strict';
(() => {
  const pending = new Map();

  async function open({ background = false, previousID = null } = {}) {
    if (window.DEVSHELL?.localShellSupported === false) { toast('当前平台不支持本地终端'); return null; }
    const previous = previousID ? sessions.get(previousID) : null;
    if (previous?.detaching || previous?.handoffProvisional) { toast('⏳  此终端正在交接窗口，请稍后再试'); return null; }
    // 伪配置只登记在 temporaryProfiles（不进入服务器管理树），供标签标题、
    // 双击重连与断开提示复用既有按 profile 查找的代码路径。
    const profileID = crypto.randomUUID();
    temporaryProfiles.set(profileID, { id: profileID, name: '本地 Shell', protocol: 'local', host: '', user: '', port: 0 });
    const startPage = !background && !previous ? window.DengStartTabs?.take() : null;
    const tabOrder = previous?.tabOrder ?? startPage?.tabOrder ?? nextSessionOrder++;
    const state = makeSessionState({ id: `pending:${profileID}:${crypto.randomUUID()}`, profileId: profileID, home: '~', localShell: true, sftpAvailable: false });
    Object.assign(state, { tabOrder, connected: false, localOnly: true, pendingConnection: true, announceConnectionReady: true, connectionAbort: new AbortController() });
    const alive = () => !state.closed && sessions.get(state.id) === state;
    const forget = () => { if (!sessions.get(state.id)) temporaryProfiles.delete(profileID); };
    // 重连复用 SSH 的屏幕保留流程：旧标签的内容保持到新会话接管。
    const previousClose = previous ? preserveReconnectTerminal(previous, state) : Promise.resolve();
    sessions.set(state.id, state);
    try {
      createTerminal(state);
      if (!background || (!activeID && !window.DengRDP?.active())) activate(state.id); else renderTabs();
      if (!background) setDrawer(false);
      await previousClose;
      if (!alive()) { forget(); return null; }
      await restoreReconnectTerminal(state);
      if (!alive()) { forget(); return null; }
      showConnectionProgress(state, '正在启动本地 Shell…');
      const info = await post('/api/local-shell/sessions', {});
      if (!info) return null;
      // 标签可能在 Shell 启动期间被关闭；顺手释放已经启动的会话。
      if (!alive()) { await remove(`/api/local-shell/sessions/${info.id}`).catch(() => {}); forget(); return null; }
      const pseudo = temporaryProfiles.get(profileID);
      if (pseudo && info.name) pseudo.name = info.name;
      const pendingID = state.id, selected = activeID === pendingID;
      sessions.delete(pendingID);
      // 重连时旧会话的伪配置随旧标签一起退场，避免 temporaryProfiles 泄漏。
      if (previous && previous.profileId !== profileID) temporaryProfiles.delete(previous.profileId);
      Object.assign(state, info, { cwd: info.cwd || '~', connected: true, localOnly: false, pendingConnection: false });
      sessions.set(state.id, state); state.host.dataset.session = state.id;
      state.openTerminalSocket();
      if (selected) activate(state.id); else renderTabs();
      return state;
    } catch (error) {
      if (alive()) {
        showConnectionProgress(state, '本地 Shell 启动失败：' + (error.message || String(error)), true);
        toast(`❌  本地 Shell：${error.message || error}`);
      }
      return null;
    } finally {
      if (alive() && state.localOnly) { state.pendingConnection = false; renderTabs(); if (current() === state) renderSessionInfo(); }
      forget();
      updateStatus();
    }
  }

  async function close(id) {
    const state = sessions.get(id); if (!state) return;
    dropSessionView(id);
    temporaryProfiles.delete(state.profileId);
    if (state.localOnly) return;
    // 关闭终端中继本身就会结束 Shell；DELETE 兜住从未挂上终端的窗口。
    await remove(`/api/local-shell/sessions/${id}`).catch(() => {});
  }

  window.DengLocalShell = {
    open() { return open(); },
    reconnect(options = {}) {
      const key = options.previousID ? `reconnect:${options.previousID}` : 'new:local-shell';
      if (pending.has(key)) return pending.get(key);
      const request = open(options).finally(() => pending.delete(key));
      pending.set(key, request); return request;
    },
    has: id => sessions.get(id)?.localShell === true,
    close,
  };
})();
