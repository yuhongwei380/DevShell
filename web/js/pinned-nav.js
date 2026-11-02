'use strict';
// Pinned connections: a bookmark button in the session bar (right of the
// server-manager button) drops a vertical list of the pinned connections
// under it. Nothing is tiled in the header, so the shell fills the window
// from the top; opening a connection closes the list, like any menu.
// Pin the current session (saving a quick-connect session first) and it
// appears in the list; each click opens a fresh session tab, like
// double-clicking the server list.
// The id list lives in appearance.layout under one key, whitelisted by the
// backend layout validator (validSavedLayoutKey in preferences.go).
window.DengPinnedNav = (() => {
  const LAYOUT_KEY = 'dengshell.pinned.v1';
  let bar, toggle, pinButton, pinned = [];
  let cancelDrag, reorderPending = false, reorderSave = Promise.resolve(), suppressClickUntil = 0, expanded = false;
  function readPinned() {
    const raw = appearance.layout?.[LAYOUT_KEY];
    pinned = Array.isArray(raw) ? raw.filter(id => typeof id === 'string') : [];
  }
  function isPinned(id) { return pinned.includes(id); }
  async function writePinned() {
    await persistAppearance({ layout: { [LAYOUT_KEY]: pinned } });
  }
  function reorder(source, target, after) {
    if (reorderPending || source === target) return;
    readPinned();
    const previous = [...pinned];
    if (!previous.includes(source) || !previous.includes(target)) return;
    const next = previous.filter(id => id !== source);
    next.splice(next.indexOf(target) + (after ? 1 : 0), 0, source);
    if (next.every((id, index) => id === previous[index])) return;
    pinned = next;
    reorderPending = true;
    reorderSave = (async () => {
      try {
        const saving = writePinned(); // persistAppearance applies the layout immediately.
        render();
        await saving;
      } catch (error) {
        // Restore just this key, preserving unrelated appearance changes.
        if (JSON.stringify(appearance.layout?.[LAYOUT_KEY]) === JSON.stringify(next)) {
          appearance.layout[LAYOUT_KEY] = previous;
        }
        throw error;
      } finally {
        reorderPending = false;
        render(); reflect();
      }
    })();
    safe(() => reorderSave)();
  }
  // Vertical drag-and-drop ordering within the open list. The drop gap is
  // the half of the hovered row below (or above) its middle.
  function bindSorting(item, open, profile) {
    item.dataset.pinnedId = profile.id;
    item.addEventListener('dragstart', event => event.preventDefault());
    open.addEventListener('pointerdown', event => {
      if (event.button !== 0 || event.isPrimary === false || reorderPending) return;
      cancelDrag?.();
      let moved = false, position, frame, marked;
      let x = event.clientX, y = event.clientY;
      const clearMark = () => { marked?.classList.remove('pinned-drop-before', 'pinned-drop-after'); marked = null; };
      const cleanup = () => {
        if (moved) suppressClickUntil = Date.now() + 600;
        cancelAnimationFrame(frame); clearMark();
        item.classList.remove('pinned-drag-source');
        document.documentElement.classList.remove('pinned-sorting');
        document.removeEventListener('pointermove', move);
        document.removeEventListener('pointerup', up);
        document.removeEventListener('pointercancel', cancel);
        document.removeEventListener('keydown', keydown, true);
        window.removeEventListener('blur', cancel);
        open.removeEventListener('lostpointercapture', cancel);
        if (open.hasPointerCapture(event.pointerId)) open.releasePointerCapture(event.pointerId);
        if (cancelDrag === cancel) cancelDrag = null;
      };
      const cancel = () => { cleanup(); };
      const keydown = e => { if (e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); cancel(); } };
      const update = () => {
        clearMark(); position = null;
        const hit = document.elementFromPoint(x, y);
        if (!hit || !bar.contains(hit)) return;
        let target = hit.closest('[data-pinned-id]');
        if (!target) {
          const candidates = [...bar.querySelectorAll('.pinned-item')];
          target = candidates.find(el => y < el.getBoundingClientRect().bottom) || candidates.at(-1);
        }
        if (!target) return;
        const r = target.getBoundingClientRect();
        const after = y >= (r.top + r.bottom) / 2;
        position = { id: target.dataset.pinnedId, after };
        marked = target;
        marked.classList.add(after ? 'pinned-drop-after' : 'pinned-drop-before');
      };
      const tick = () => {
        const r = bar.getBoundingClientRect();
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
          const edge = Math.min(24, r.height / 3);
          bar.scrollTop += (y < r.top + edge ? -5 : y > r.bottom - edge ? 5 : 0) / effectiveScale;
        }
        update(); frame = requestAnimationFrame(tick);
      };
      const move = e => {
        if (e.pointerId !== event.pointerId) return;
        x = e.clientX; y = e.clientY;
        if (!moved && Math.hypot(x - event.clientX, y - event.clientY) < 6) return;
        if (!moved) {
          moved = true;
          open.setPointerCapture(event.pointerId);
          item.classList.add('pinned-drag-source');
          document.documentElement.classList.add('pinned-sorting');
          frame = requestAnimationFrame(tick);
        }
        e.preventDefault(); update();
      };
      const up = e => {
        if (e.pointerId !== event.pointerId) return;
        x = e.clientX; y = e.clientY;
        if (moved) { e.preventDefault(); update(); }
        const drop = moved && position;
        cleanup();
        if (drop) reorder(profile.id, drop.id, drop.after);
      };
      cancelDrag = cancel;
      document.addEventListener('pointermove', move);
      document.addEventListener('pointerup', up);
      document.addEventListener('pointercancel', cancel);
      document.addEventListener('keydown', keydown, true);
      window.addEventListener('blur', cancel);
      open.addEventListener('lostpointercapture', cancel);
    });
  }
  const profileSubtitle = profile => profile.protocol === 'serial' ? window.DengSerial.label(profile) : `${profile.user}@${profile.host}:${profile.port}`;
  function openConnection(profile) {
    // Same semantics as double-clicking in the server list: every click opens a
    // fresh session tab, even when a session for this profile already exists
    // (activating the old one made repeat clicks look dead). connect() coalesces
    // only in-flight requests, so accidental double clicks cannot stack up.
    connect(profile.id);
  }
  function removeButtonFor(profile) {
    const remove = node('button', 'pinned-item-remove');
    remove.type = 'button';
    remove.append(icon('close'));
    remove.title = '关闭（取消固定）';
    remove.setAttribute('aria-label', `取消固定 ${profile.name}`);
    remove.onclick = event => { event.stopPropagation(); safe(() => confirmUnpin(profile))(); };
    return remove;
  }
  // Unpinning is destructive enough to ask first: the close button is the only
  // unpin affordance and always routes through the shared confirmation dialog.
  async function confirmUnpin(profile) {
    if (!await ask({ title: `取消固定 ${profile.name}？`, description: `将从链接栏移除 ${profile.name}（${profileSubtitle(profile)}）。连接配置与已保存数据不受影响，之后可重新固定。`, confirm: '取消固定' })) return;
    await unpin(profile.id);
  }
  // Right-click renames in place: the name is the same field the server
  // manager edits, so the saved profile is re-posted with only the name
  // changed. Secrets and proxy passwords round-trip through the public view
  // untouched (store.Save keeps them while the auth shape is unchanged).
  async function renameProfile(profile) {
    const answer = await ask({ title: '重命名连接', description: `修改「${profile.name}」的名称，与服务器管理里该连接的名称同步。`, input: true, value: profile.name, confirm: '保存' });
    if (answer === null) return;
    const name = String(answer).trim();
    if (!name) { toast('名称不能为空'); return; }
    if (name === profile.name) return;
    const saved = await post('/api/profiles', { ...profile, name, clearSecret: false });
    const index = profiles.findIndex(entry => entry.id === saved.id);
    if (index >= 0) profiles[index] = saved; else profiles.push(saved);
    renderConnections(); renderTabs(); renderSessionInfo();
    render(); reflect();
    toast(`已重命名为「${saved.name}」`);
  }
  const itemTitle = profile => `${profile.name} · ${profileSubtitle(profile)}\n拖拽调整顺序 · 左键新建会话 · 右键重命名 · × 关闭（取消固定需确认）`;
  function itemFor(profile) {
    const item = node('span', 'pinned-item');
    const open = node('button', 'pinned-item-open');
    open.type = 'button';
    open.append(icon('server'), node('span', '', profile.name));
    open.onclick = () => { setExpanded(false); openConnection(profile); };
    item.oncontextmenu = event => { event.preventDefault(); safe(() => renameProfile(profile))(); };
    item.append(open, removeButtonFor(profile));
    item.title = itemTitle(profile);
    bindSorting(item, open, profile);
    return item;
  }
  function anchorPanel() {
    if (!expanded || !toggle || !bar) return;
    const r = toggle.getBoundingClientRect();
    const top = r.bottom / effectiveScale + 7;
    bar.style.top = top + 'px';
    bar.style.left = Math.max(8, Math.min(r.left / effectiveScale, logicalWidth() - bar.offsetWidth - 8)) + 'px';
    bar.style.maxHeight = Math.min(420, Math.max(0, logicalHeight() - top - 8)) + 'px';
  }
  function setExpanded(on) {
    expanded = !!on;
    if (!toggle || !bar) return;
    if (!expanded) cancelDrag?.();
    bar.hidden = !expanded;
    toggle.setAttribute('aria-expanded', String(expanded));
    toggle.classList.toggle('open', expanded);
    const label = expanded ? '收起链接栏' : '链接栏';
    toggle.title = label;
    toggle.setAttribute('aria-label', label);
    if (expanded) { bar.scrollTop = 0; anchorPanel(); }
  }
  function emptyRow() {
    return node('div', 'pinned-empty', '还没有固定的连接。连接服务器后，点会话工具栏里的「固定」即可把它放进来。');
  }
  function render() {
    if (!bar) return;
    cancelDrag?.();
    readPinned();
    bar.hidden = !expanded;
    const items = pinned.map(id => connectionProfile(id)).filter(Boolean).map(itemFor);
    // The expand button is always available; the dropdown simply shows an
    // empty-state hint until the first connection is pinned.
    bar.replaceChildren(...(items.length ? items : [emptyRow()]));
    if (toggle) toggle.hidden = false;
    if (expanded) anchorPanel();
  }
  async function pin(id) { await reorderSave.catch(() => {}); pinned = [...pinned, id]; await writePinned(); render(); reflect(); }
  async function unpin(id) { await reorderSave.catch(() => {}); pinned = pinned.filter(x => x !== id); await writePinned(); render(); reflect(); toast('已从链接栏取消固定'); }
  async function toggleCurrent() {
    const state = current(), profile = profileFor(state);
    if (!state || !profile) return;
    if (isPinned(profile.id)) { await unpin(profile.id); return; }
    let note = '已固定到链接栏';
    if (temporaryProfiles.has(profile.id)) {
      // Pinning a quick-connect session persists it first, like "保存连接",
      // carrying the credentials used this run so the pin reconnects silently.
      // The credential lives in this window's memory only, so say out loud when
      // there was nothing to carry instead of saving a silently password-less row.
      const remembered = profile.auth !== 'agent' && credentials.has(profile.id);
      const saved = await post(`/api/quick-connect/${profile.id}/save`, { name: profile.name, group: '快速连接', notes: '', remember: remembered, secret: credentials.get(profile.id) || '' });
      temporaryProfiles.delete(profile.id);
      profiles = profiles.filter(p => p.id !== saved.id);
      profiles.push(saved);
      renderConnections(); renderTabs();
      // The session is no longer temporary even if pinning below fails; the
      // session tab menu rebuilds its "保存连接" entry on every open.
      if (!saved.hasSecret) note = '已固定到链接栏；这个连接没有保存凭据（本窗口未输入过密码），下次连接仍需输入';
    }
    await pin(profile.id);
    toast(note);
  }
  function reflect() {
    readPinned();
    if (toggle) toggle.hidden = false;
    if (!pinButton) return;
    const state = current(), profile = profileFor(state);
    pinButton.hidden = !state || !profile;
    if (!state || !profile) return;
    const on = isPinned(profile.id);
    pinButton.setAttribute('aria-pressed', String(on));
    pinButton.classList.toggle('active', on);
    pinButton.title = on ? '从链接栏取消固定' : temporaryProfiles.has(profile.id) ? '保存并固定到链接栏' : '固定到链接栏';
  }
  document.addEventListener('DOMContentLoaded', () => {
    bar = $('#pinned-bar');
    toggle = $('#pinned-toggle');
    toggle.onclick = () => setExpanded(!expanded);
    bar.addEventListener('pointerdown', () => { suppressClickUntil = 0; }, true);
    bar.addEventListener('click', event => {
      if (Date.now() < suppressClickUntil && event.detail !== 0) {
        event.preventDefault(); event.stopImmediatePropagation();
      }
    }, true);
    pinButton = node('button', 'terminal-popout');
    pinButton.id = 'pin-session';
    pinButton.hidden = true;
    pinButton.append(icon('bookmark'), node('span', '', '固定'));
    pinButton.onclick = () => safe(toggleCurrent)();
    $('#toggle-sftp').before(pinButton);
    render(); reflect();
    let frame;
    const scheduleLayout = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => anchorPanel());
    };
    new ResizeObserver(scheduleLayout).observe(bar);
    window.addEventListener('resize', scheduleLayout);
    document.fonts.ready.then(scheduleLayout);
    document.fonts.addEventListener('loadingdone', scheduleLayout);
    document.addEventListener('click', event => {
      // Confirmation and rename dialogs may open while the list is showing;
      // interacting with them must not count as an outside click.
      if (expanded && !bar.contains(event.target) && !toggle.contains(event.target) && !event.target.closest('dialog[open]')) setExpanded(false);
    }, true);
    document.addEventListener('keydown', event => {
      if (event.key !== 'Escape') return;
      if (expanded && !cancelDrag) { setExpanded(false); toggle.focus(); }
    });
  }, { once: true });
  return { render, reflect, renameProfile };
})();
