/* 串口终端：DengSerial 只负责枚举本机串口并创建串口会话；
   终端渲染、输入与断开处理复用 app.js 中与 SSH 相同的 WebSocket 中继协议。
   串口没有 SFTP、监控和窗口交接，state.serial 是各处的分流标记。 */
'use strict';
(() => {
  const pending = new Map();

  function serialLabel(profile) {
    const port = profile?.serialPort || profile?.host || '串口';
    const baud = profile?.baudRate || 115200;
    const parity = { none: 'N', odd: 'O', even: 'E', mark: 'M', space: 'S' }[profile?.parity || 'none'] || 'N';
    const stop = profile?.stopBits || 1;
    const flow = { none: '', rtscts: ' · RTS/CTS', xonxoff: ' · XON/XOFF' }[profile?.flowControl || 'none'] || '';
    return `${port} · ${baud}bps ${parity}${stop}${flow}`;
  }

  async function loadPorts() {
    const select = document.getElementById('serial-port-choices');
    const note = document.getElementById('serial-ports-note');
    if (!select) return;
    select.disabled = true;
    try {
      const result = await api('/api/serial/ports');
      const ports = result.ports || [];
      select.replaceChildren(new Option(ports.length ? '从检测到的串口中选择' : '未检测到串口', ''));
      for (const port of ports) select.append(new Option(port, port));
      if (note) note.textContent = ports.length ? `检测到 ${ports.length} 个本机串口：${ports.join('、')}` : '未检测到本机串口，可手动填写设备名称。';
    } catch (error) {
      select.replaceChildren(new Option('检测失败', ''));
      if (note) note.textContent = `串口检测失败：${error.message || error}`;
    } finally {
      select.disabled = false;
    }
  }

  async function open(profileID, { background = false, previousID = null } = {}) {
    const profile = connectionProfile(profileID); if (!profile) return null;
    const previous = previousID ? sessions.get(previousID) : null;
    if (previous?.detaching || previous?.handoffProvisional) { toast('⏳  此终端正在交接窗口，请稍后再试'); return null; }
    const startPage = !background && !previous ? window.DengStartTabs?.take() : null;
    const tabOrder = previous?.tabOrder ?? startPage?.tabOrder ?? nextSessionOrder++;
    const state = makeSessionState({ id: `pending:${profileID}:${crypto.randomUUID()}`, profileId: profileID, home: '/', serial: true, sftpAvailable: false });
    Object.assign(state, { tabOrder, connected: false, localOnly: true, pendingConnection: true, announceConnectionReady: true, connectionAbort: new AbortController() });
    const alive = () => !state.closed && sessions.get(state.id) === state;
    connecting.add(profileID);
    // Reconnect reuses the SSH screen-preservation helpers: the old tab keeps
    // its scrollback while the new serial session opens in its place.
    const previousClose = previous ? preserveReconnectTerminal(previous, state) : Promise.resolve();
    sessions.set(state.id, state);
    try {
      createTerminal(state);
      if (!background || (!activeID && !window.DengRDP?.active())) activate(state.id); else renderTabs();
      if (!background) setDrawer(false);
      renderConnections();
      await previousClose;
      if (!alive()) return null;
      await restoreReconnectTerminal(state);
      if (!alive()) return null;
      showConnectionProgress(state, `打开串口 ${profile.serialPort || profile.host}…`);
      const info = await post('/api/serial/sessions', { profileId: profileID });
      if (!info) return null;
      // The tab may close while the port is opening; release the physical port.
      if (!alive()) { await remove(`/api/serial/sessions/${info.id}`).catch(() => {}); return null; }
      const pendingID = state.id, selected = activeID === pendingID;
      sessions.delete(pendingID);
      Object.assign(state, info, { cwd: '/', connected: true, localOnly: false, pendingConnection: false });
      sessions.set(state.id, state); state.host.dataset.session = state.id;
      state.openTerminalSocket();
      if (selected) activate(state.id); else renderTabs();
      safe(refreshServerManagerHistory)();
      return state;
    } catch (error) {
      if (alive()) {
        showConnectionProgress(state, '打开串口失败：' + (error.message || String(error)), true);
        toast(`❌  ${profile.name}：${error.message || error}`);
      }
      return null;
    } finally {
      connecting.delete(profileID);
      if (alive() && state.localOnly) { state.pendingConnection = false; renderTabs(); if (current() === state) renderSessionInfo(); }
      renderConnections(); updateStatus();
    }
  }

  async function close(id) {
    const state = sessions.get(id); if (!state) return;
    dropSessionView(id);
    if (state.localOnly) return;
    // Closing the terminal relay already releases the port; DELETE covers the
    // window where the session exists but its terminal never attached.
    await remove(`/api/serial/sessions/${id}`).catch(() => {});
  }

  document.addEventListener('DOMContentLoaded', () => {
    const choices = document.getElementById('serial-port-choices');
    choices.onchange = () => { if (choices.value) document.getElementById('connection-form').elements.serialPort.value = choices.value; };
    document.getElementById('refresh-serial-ports').onclick = safe(loadPorts);
  }, { once: true });

  window.DengSerial = {
    connect(profileID, options = {}) {
      const key = options.previousID ? `reconnect:${options.previousID}` : `new:${profileID}`;
      if (pending.has(key)) return pending.get(key);
      const request = open(profileID, options).finally(() => pending.delete(key));
      pending.set(key, request); return request;
    },
    has: id => sessions.get(id)?.serial === true,
    close, loadPorts, label: serialLabel,
  };
})();
