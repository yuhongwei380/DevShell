/* telnet 本机 TCP 会话（候选栏 / 本机提示符 `telnet 主机 [端口]`，默认 23）：
   在当前标签拨号，终端渲染、输入与断开处理复用与 SSH、串口、本地终端一致的
   WebSocket 中继协议；state.telnet 是各处分流标记。断开后的「重新连接」或再
   输入一次 telnet = 用同一主机端口在本标签重拨。开关在设置 → 其他功能。 */
'use strict';
(() => {
  const pending = new Map();
  function parse(command) {
    const match = command.trim().match(/^telnet\s+(\S+?)(?:\s+(\d{1,5}))?\s*$/i);
    if (!match) return null;
    const port = match[2] ? Number(match[2]) : 23;
    if (port < 1 || port > 65535) return null;
    return { host: match[1], port };
  }
  async function open({ host, port, background = false, previousID = null } = {}) {
    if (!host || !port) return null;
    const previous = previousID ? sessions.get(previousID) : null;
    if (previous?.detaching || previous?.handoffProvisional) { toast('⏳  此终端正在交接窗口，请稍后再试'); return null; }
    // 伪配置只登记在 temporaryProfiles：标签标题、双击重连与断开提示复用既有
    // 按 profile 查找的代码路径（protocol 'tcp' 在 connect() 里分流回这里）。
    const profileID = crypto.randomUUID();
    temporaryProfiles.set(profileID, { id: profileID, name: `telnet ${host}:${port}`, protocol: 'tcp', host, user: '', port });
    const startPage = !background && !previous ? window.DengStartTabs?.take() : null;
    const tabOrder = previous?.tabOrder ?? startPage?.tabOrder ?? nextSessionOrder++;
    const state = makeSessionState({ id: `pending:${profileID}:${crypto.randomUUID()}`, profileId: profileID, home: '~', telnet: true, sftpAvailable: false });
    Object.assign(state, { tabOrder, connected: false, localOnly: true, pendingConnection: true, announceConnectionReady: true, connectionAbort: new AbortController() });
    const alive = () => !state.closed && sessions.get(state.id) === state;
    const forget = () => { if (!sessions.get(state.id)) temporaryProfiles.delete(profileID); };
    // 重拨复用 SSH 的屏幕保留流程：旧标签的内容保持到新会话接管。
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
      showConnectionProgress(state, `正在拨号 ${host}:${port}…`);
      const info = await post('/api/tcp-sessions', { host, port });
      if (!info) return null;
      // 标签可能在拨号期间被关闭；顺手释放已经建立的连接。
      if (!alive()) { await remove(`/api/tcp-sessions/${info.id}`).catch(() => {}); forget(); return null; }
      const pendingID = state.id, selected = activeID === pendingID;
      sessions.delete(pendingID);
      // 重拨时旧会话的伪配置随旧标签一起退场，避免 temporaryProfiles 泄漏。
      if (previous && previous.profileId !== profileID) temporaryProfiles.delete(previous.profileId);
      // TcpSessionInfo.host 是主机名，会撞掉 createTerminal 建立的 state.host DOM 容器，
      // 导致后续 state.host.dataset 抛错、已建立的连接被挂起；第三参数把容器夺回来。
      Object.assign(state, info, { host: state.host, cwd: '~', connected: true, localOnly: false, pendingConnection: false });
      sessions.set(state.id, state); state.host.dataset.session = state.id;
      state.openTerminalSocket();
      if (selected) activate(state.id); else renderTabs();
      return state;
    } catch (error) {
      if (alive()) {
        showConnectionProgress(state, `拨号 ${host}:${port} 失败：` + (error.message || String(error)), true);
        toast(`❌  telnet ${host}:${port}：${error.message || error}`);
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
    // 关闭终端中继本身就会断开连接；DELETE 兜住从未挂上终端的窗口。
    await remove(`/api/tcp-sessions/${id}`).catch(() => {});
  }
  window.DengTcpSession = {
    parse,
    open(target, options = {}) { return open({ ...target, ...options }); },
    reconnect(options = {}) {
      // connect() 的 'tcp' 分支走这里：从伪配置取回主机与端口重拨。
      const profile = options.profileId ? connectionProfile(options.profileId) : null;
      const target = profile?.protocol === 'tcp' ? { host: profile.host, port: profile.port } : null;
      if (!target) return Promise.resolve(null);
      const key = options.previousID ? `reconnect:${options.previousID}` : `new:${target.host}:${target.port}`;
      if (pending.has(key)) return pending.get(key);
      const request = open({ ...target, ...options }).finally(() => pending.delete(key));
      pending.set(key, request); return request;
    },
    has: id => sessions.get(id)?.telnet === true,
    close,
  };
})();
