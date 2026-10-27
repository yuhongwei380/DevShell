'use strict';

// Tab completion for the bottom command bar. Completions exist only while the
// bar is a local prompt —— the welcome / start page, or a tab whose remote
// shell has exited. A connected session keeps plain Tab: real completion
// belongs to the remote shell, never to DevShell.
window.DengCommandCompletion = (() => {
  // Mirrors sessionLocalCommand() and the local-prompt placeholder; welcome /
  // start pages have no tab to reconnect or close, so help / reconnect / exit
  // only appear while a session object exists.
  const sessionCommands = [
    { name: 'ssh', usage: '[-vvv] 用户@主机', note: '在本标签连接新地址' },
    { name: 'reconnect', note: '用原地址重新连接本标签' },
    { name: 'ping', usage: '主机 [-t]', note: '本机探测' },
    { name: 'traceroute', usage: '主机', note: '本机路由追踪' },
    { name: 'nslookup', usage: '主机', note: '本机 DNS 解析' },
    { name: 'dig', usage: '主机 [类型]', note: '本机 DNS 查询' },
    { name: 'curl', usage: '[-I] 网址', note: '本机 http/https 请求' },
    { name: 'help', note: '显示本机命令用法' },
    { name: 'exit', note: '关闭本标签' },
  ];
  const welcomeCommands = sessionCommands.filter(command => ['ssh', 'ping', 'traceroute', 'nslookup', 'dig', 'curl'].includes(command.name));
  // Same local-prompt test as renderSessionInfo(): while the bar is neither a
  // start page nor a live local prompt, Tab keeps its old behaviour.
  function commandsFor(state) {
    if (!state) return welcomeCommands;
    if (state.connected || state.pendingConnection || state.disconnecting || state.detaching || state.handoffProvisional || state.ownershipUncertain) return null;
    return sessionCommands;
  }
  // Only the first token of the caret's line is a command word; the rest are
  // arguments DevShell never rewrites. A partial token after the caret (caret
  // inside the word) is consumed by the replacement as well.
  function wordAt(value, caret) {
    const before = value.slice(0, caret);
    const lineStart = before.lastIndexOf('\n') + 1;
    const wordStart = Math.max(lineStart, before.search(/[^\s]*$/));
    if (before.slice(lineStart, wordStart).trim()) return null;
    const tail = value.slice(caret).match(/^[^\s]*/)[0];
    return { word: before.slice(wordStart), start: wordStart, end: caret + tail.length };
  }
  function commonPrefix(names) {
    let prefix = names[0] || '';
    for (const name of names.slice(1)) while (prefix && !name.startsWith(prefix)) prefix = prefix.slice(0, -1);
    return prefix;
  }
  let popup = null, session = null;
  function input() { return document.getElementById('command-input'); }
  function ensure() {
    if (popup?.isConnected) return popup;
    const bar = document.querySelector('.terminal-command-bar');
    if (!bar) return null;
    popup = node('div', 'command-completion-popup'); popup.id = 'command-completion'; popup.hidden = true;
    popup.setAttribute('role', 'listbox'); popup.setAttribute('aria-label', '本机功能命令候选');
    // Keep the textarea focused while a row is clicked; the click commits.
    popup.addEventListener('pointerdown', event => event.preventDefault());
    popup.addEventListener('click', event => {
      event.preventDefault(); event.stopPropagation();
      const row = event.target.closest('[data-index]');
      const command = row && session?.list[Number(row.dataset.index)];
      const area = input(), at = command && wordAt(area.value, area.selectionStart);
      if (!at) return hide();
      area.setRangeText(command.name, at.start, at.end, 'end');
      resizeCommandInput(); hide(); area.focus();
    });
    bar.append(popup);
    return popup;
  }
  function show(list, active) {
    if (!ensure()) return;
    session = { list, index: active };
    popup.replaceChildren(...list.map((command, index) => {
      const row = node('div', 'command-completion-option');
      row.id = `command-completion-${index}`; row.dataset.index = String(index); row.setAttribute('role', 'option');
      row.append(node('code', 'command-completion-name', command.name), node('code', 'command-completion-usage', command.usage || ''), node('span', 'command-completion-note', command.note || ''));
      return row;
    }));
    popup.hidden = false;
    mark(active);
  }
  function mark(index) {
    if (!popup || popup.hidden) return;
    [...popup.children].forEach((row, i) => { row.classList.toggle('is-active', i === index); row.setAttribute('aria-selected', String(i === index)); });
    const area = input();
    area.setAttribute('aria-expanded', 'true');
    if (index >= 0) {
      area.setAttribute('aria-activedescendant', `command-completion-${index}`);
      popup.children[index]?.scrollIntoView({ block: 'nearest' });
    } else area.removeAttribute('aria-activedescendant');
  }
  function hide() {
    session = null;
    if (popup && !popup.hidden) {
      popup.hidden = true;
      const area = input();
      area.removeAttribute('aria-expanded'); area.removeAttribute('aria-activedescendant');
    }
  }
  function onKeyDown(event) {
    if (event.isComposing) return;
    const area = event.target;
    // Enter sends, ↑ / ↓ switch to history recall, Escape dismisses: in every
    // case the candidate box ends here.
    if (event.key === 'Enter' || event.key === 'Escape' || event.key === 'ArrowUp' || event.key === 'ArrowDown') { hide(); return; }
    if (event.key !== 'Tab' || event.ctrlKey || event.altKey || event.metaKey) return;
    const commands = commandsFor(current());
    if (!commands) return;
    if (area.selectionStart !== area.selectionEnd) { hide(); return; }
    const at = wordAt(area.value, area.selectionStart);
    if (!at) return;
    if (session?.list?.length) {
      // Keep cycling the same candidate set; each press replaces the current
      // command word with the next or previous name.
      event.preventDefault();
      const total = session.list.length;
      session.index = (session.index + (event.shiftKey ? -1 : 1) + total) % total;
      area.setRangeText(session.list[session.index].name, at.start, at.end, 'end');
      resizeCommandInput(); mark(session.index);
      return;
    }
    const lower = at.word.toLowerCase();
    const matches = commands.filter(command => command.name.startsWith(lower));
    if (!matches.length) return; // nothing to offer: let Tab behave as before
    event.preventDefault();
    if (matches.length === 1) {
      area.setRangeText(matches[0].name, at.start, at.end, 'end');
      resizeCommandInput(); show(matches, 0);
      return;
    }
    const prefix = commonPrefix(matches.map(command => command.name));
    if (prefix.length > at.word.length) {
      area.setRangeText(prefix, at.start, at.end, 'end');
      resizeCommandInput();
    }
    show(matches, -1);
  }
  function init() {
    const area = input();
    if (!area) return;
    area.addEventListener('keydown', onKeyDown);
    // Any typed change invalidates the candidate set. History recall assigns
    // .value directly, so its ↑ / ↓ keys hide the box in onKeyDown instead.
    area.addEventListener('input', hide);
    area.addEventListener('blur', hide);
    window.addEventListener('dengshell:session-context-change', hide);
  }
  document.addEventListener('DOMContentLoaded', init, { once: true });
  return { commandsFor, wordAt, commonPrefix, hide };
})();
