'use strict';
// Xshell-style local prompt: while no session is connected, network commands
// typed in the command bar run against the local machine and print straight
// into the terminal area instead of opening the quick-connect dialog. Supported:
// ping 主机 [-t], traceroute 主机 (hops appear live, one per line), nslookup 主机,
// dig 主机 [类型], curl [-I] 网址 (scheme-less targets default to http, like the
// real curl), tcping 主机 端口 [-t] (TCP handshake probe). telnet 主机 [端口] is
// recognized here only for usage help; call sites dial it as a tab session
// before reaching this module. Every command is interruptible: Ctrl+C (without
// a text selection) aborts the request, and the backend cancels the child
// process or lookup.
window.DengLocalPrompt = (() => {
  let view = null, generation = 0;
  const commands = /^(ping|traceroute|tracert|nslookup|dig|curl|tcping|telnet)(?:\s|$)/i;
  function ensure() {
    if (view?.isConnected) return view;
    view = node('pre', 'local-prompt-output');
    view.id = 'local-prompt-output';
    view.setAttribute('role', 'log');
    view.hidden = true;
    $('#terminal-output').append(view);
    return view;
  }
  function trimHistory(box) { while (box.children.length > 80) box.firstChild.remove(); }
  // A sink writes one chunk and returns an in-place updater, or null when the
  // output is append-only. The scratchpad supports updates; a session terminal
  // appends to scrollback.
  function scratchSink(box) {
    return (text, kind) => {
      const span = node('span', kind === 'cmd' ? 'local-prompt-line' : '');
      span.textContent = text;
      box.append(span);
      trimHistory(box);
      box.scrollTop = box.scrollHeight;
      return value => { span.textContent = value; box.scrollTop = box.scrollHeight; };
    };
  }
  function termSink(term) {
    return (text, kind) => {
      // xterm treats a bare LF as "move down, keep the column": multi-line
      // payloads (ping output) must use CRLF, or every line starts where the
      // previous one ended and the output staircases to the right.
      const body = text.replace(/[\r\n]+$/, '').replace(/\r?\n/g, '\r\n');
      term.writeln(kind === 'cmd' ? `\x1b[38;5;114m${body}\x1b[0m` : `\x1b[38;5;245m${body}\x1b[0m`);
      return null;
    };
  }
  function parsePing(command) {
    const tokens = command.trim().replace(/^ping(?:\s+|$)/i, '').trim().split(/\s+/).filter(Boolean);
    let continuous = false;
    const hosts = [];
    for (const token of tokens) {
      if (token === '-t' || token === '-T') continuous = true;
      else hosts.push(token);
    }
    if (hosts.length !== 1 || hosts[0].startsWith('-')) return null;
    return { host: hosts[0], continuous };
  }
  // Console-style interrupt: Ctrl+C aborts the running probe unless it would
  // copy a text selection. The aborted request cancels the backend context,
  // which terminates the child process or the in-flight lookup.
  function isInterruptKey(event) { return (event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'c'; }
  async function withInterrupt(task) {
    const controller = new AbortController();
    let interrupted = false;
    const onKey = event => {
      if (!isInterruptKey(event) || window.getSelection()?.toString()) return;
      event.preventDefault();
      interrupted = true;
      controller.abort();
    };
    document.addEventListener('keydown', onKey);
    try { await task(controller.signal); } finally { document.removeEventListener('keydown', onKey); }
    return interrupted;
  }
  // Probes always go over the local HTTP API: the native bridge cannot abort
  // or stream a call, and these endpoints are plain JSON/NDJSON either way.
  async function postWithSignal(path, body, signal) {
    const response = await fetch(endpoint(path), { method: 'POST', signal, cache: 'no-store', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': boot.token }, body: JSON.stringify(body) });
    const data = await response.json().catch(() => null);
    if (data?.code === 'DENGSHELL_LOCKED') window.DengSecurityLock?.blocked();
    if (!response.ok) throw Object.assign(new Error(data?.error || `请求失败 (${response.status})`), { code: data?.code, hostKey: data?.hostKey });
    return data;
  }
  // NDJSON frames: {"c": "..."} per output line, then {"done": true}. A server
  // without streaming support answers with the single JSON payload instead.
  async function streamProbe(path, body, signal, onChunk) {
    const response = await fetch(endpoint(path), { method: 'POST', signal, cache: 'no-store', headers: { 'Content-Type': 'application/json', 'X-CloudShell-Token': boot.token, 'X-DevShell-Probe-Stream': '1' }, body: JSON.stringify(body) });
    if (!(response.headers.get('content-type') || '').includes('ndjson')) {
      const data = await response.json().catch(() => null);
      if (data?.code === 'DENGSHELL_LOCKED') window.DengSecurityLock?.blocked();
      if (!response.ok) throw Object.assign(new Error(data?.error || `请求失败 (${response.status})`), { code: data?.code, hostKey: data?.hostKey });
      if (typeof data?.output === 'string') onChunk(`${data.output}\n`);
      return;
    }
    if (!response.ok) throw new Error(`请求失败 (${response.status})`);
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      buffer += decoder.decode(chunk.value, { stream: true });
      let index;
      while ((index = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, index).trim();
        buffer = buffer.slice(index + 1);
        if (!line) continue;
        let frame = null;
        try { frame = JSON.parse(line); } catch { continue; }
        if (typeof frame.c === 'string') onChunk(frame.c);
        else if (frame.done) return;
      }
    }
  }
  async function run(command, sink) {
    if (!sink) {
      const box = ensure();
      box.hidden = false;
      $('#welcome-state').hidden = true;
      sink = scratchSink(box);
    }
    const runId = ++generation; // supersedes any running continuous probe
    sink(`[本机] $ ${command.trim()}\n`, 'cmd');
    const trimmed = command.trim();
    const verb = ((trimmed.match(/^([a-z]+)/i) || [])[1] || '').toLowerCase();
    if (verb === 'ping') { await runPing(trimmed, sink, runId); return; }
    if (verb === 'traceroute' || verb === 'tracert') { await runHostProbe('traceroute', trimmed, sink); return; }
    if (verb === 'nslookup') { await runHostProbe('nslookup', trimmed, sink); return; }
    if (verb === 'dig') { await runDig(trimmed, sink); return; }
    if (verb === 'curl') { await runCurl(trimmed, sink); return; }
    if (verb === 'tcping') { await runTcping(trimmed, sink, runId); return; }
    if (verb === 'telnet') {
      const target = window.DengTcpSession.parse(trimmed);
      if (!target) sink('telnet 用法：telnet 主机 [端口，默认 23]，将在本标签拨号。\n\n');
      else if (runId === generation) window.DengTcpSession.open(target);
      return;
    }
    sink('未知本机命令。\n\n');
  }
  async function runPing(command, sink, runId) {
    const parsed = parsePing(command);
    if (!parsed) { sink('ping 后只接受主机名或 IP，可加 -t 持续探测。\n\n'); return; }
    if (parsed.continuous) { await continuous(sink, parsed.host, runId); return; }
    const pending = sink('正在从本机探测…\n');
    const write = text => { if (pending) pending(text); else sink(text); };
    const interrupted = await withInterrupt(async signal => {
      try {
        const payload = await postWithSignal('/api/quick-connect/ping', { host: parsed.host }, signal);
        write(`${(payload?.output || '').trimEnd()}\n\n`);
      } catch (error) {
        write(error?.name === 'AbortError' ? '^C 已停止。\n\n' : `${error.message || error}\n\n`);
      }
    });
    if (!interrupted && runId !== generation) write('（已被新命令取代）\n');
  }
  async function runHostProbe(kind, command, sink) {
    const arg = command.replace(/^[a-z]+\s+/i, '').trim();
    if (!arg || /\s/.test(arg)) { sink(`${kind} 后只接受一个主机名或 IP。\n\n`); return; }
    if (kind === 'traceroute') { await runTraceroute(arg, sink); return; }
    const pending = sink('正在解析域名…\n');
    const write = text => { if (pending) pending(text); else sink(text); };
    const interrupted = await withInterrupt(async signal => {
      try {
        const payload = await postWithSignal(`/api/local-probe/${kind}`, { host: arg }, signal);
        write(`${(payload?.output || '').trimEnd()}\n\n`);
      } catch (error) {
        write(error?.name === 'AbortError' ? '^C 已停止。\n\n' : `${error.message || error}\n\n`);
      }
    });
    if (!interrupted) return;
  }
  // Streaming traceroute: the scratchpad re-renders the whole hop block in
  // place; a session terminal appends each finished line as it arrives.
  async function runTraceroute(arg, sink) {
    const header = '正在追踪路由（最多 20 跳），Ctrl+C 停止…\n';
    const pending = sink(header);
    let acc = '', carry = '', holdCR = false, failure = null;
    const display = text => text.replace(/\r\n?/g, '\n');
    const feed = chunk => {
      if (holdCR) { chunk = '\r' + chunk; holdCR = false; }
      if (chunk.endsWith('\r')) { holdCR = true; chunk = chunk.slice(0, -1); }
      if (!chunk) return;
      acc += chunk;
      if (pending) { pending(header + display(acc).trimEnd()); return; }
      const parts = (carry + chunk.replace(/\r\n/g, '\n').replace(/\r/g, '\n')).split('\n');
      carry = parts.pop();
      if (parts.length) sink(`${parts.join('\n')}\n`);
    };
    const interrupted = await withInterrupt(async signal => {
      try { await streamProbe('/api/local-probe/traceroute', { host: arg }, signal, feed); }
      catch (error) { if (error?.name !== 'AbortError') failure = error; }
    });
    if (failure) {
      const text = `${failure.message || failure}\n\n`;
      if (pending) pending(header + text); else sink(text);
      return;
    }
    if (pending) { pending(`${header}${display(acc).trimEnd()}${interrupted ? '\n^C 已停止，本机进程已终止。' : ''}\n\n`); return; }
    if (carry) { sink(`${carry.replace(/\r/g, '')}\n`); carry = ''; }
    sink(interrupted ? '^C 已停止，本机进程已终止。\n\n' : '\n');
  }
  async function runDig(command, sink) {
    const tokens = command.replace(/^dig\s+/i, '').trim().split(/\s+/).filter(Boolean);
    if (!tokens.length || tokens.length > 2) { sink('dig 用法：dig 主机 [A|AAAA|CNAME|MX|TXT|NS]。\n\n'); return; }
    const host = tokens[0];
    const type = tokens.length === 2 ? tokens[1].toUpperCase() : 'A';
    const pending = sink('正在查询 DNS…\n');
    const write = text => { if (pending) pending(text); else sink(text); };
    await withInterrupt(async signal => {
      try {
        const payload = await postWithSignal('/api/local-probe/dig', { host, type }, signal);
        write(`${(payload?.output || '').trimEnd()}\n\n`);
      } catch (error) {
        write(error?.name === 'AbortError' ? '^C 已停止。\n\n' : `${error.message || error}\n\n`);
      }
    });
  }
  async function runCurl(command, sink) {
    const tokens = command.replace(/^curl\s+/i, '').trim().split(/\s+/).filter(Boolean);
    let head = false;
    const positional = [];
    for (const token of tokens) {
      if (token === '-I' || token === '--head') head = true;
      else if (token.startsWith('-')) { sink('curl 仅支持网址与 -I（只看响应头）。\n\n'); return; }
      else positional.push(token);
    }
    if (positional.length !== 1) { sink('curl 用法：curl 网址（不带协议默认 http），可加 -I 只看响应头。\n\n'); return; }
    const pending = sink('正在请求网址…\n');
    const write = text => { if (pending) pending(text); else sink(text); };
    await withInterrupt(async signal => {
      try {
        const payload = await postWithSignal('/api/local-probe/curl', { url: positional[0], head }, signal);
        write(`${(payload?.output || '').trimEnd()}\n\n`);
      } catch (error) {
        write(error?.name === 'AbortError' ? '^C 已停止。\n\n' : `${error.message || error}\n\n`);
      }
    });
  }
  async function continuous(sink, host, runId) {
    const status = sink('持续探测中，按 Ctrl+C 停止…\n');
    const setStatus = text => { if (status) status(text); else sink(text); };
    // Ctrl+C stops the loop immediately and also aborts the in-flight probe.
    let stopped = false, inflight = null;
    const onKey = event => {
      if (!isInterruptKey(event) || window.getSelection()?.toString()) return;
      event.preventDefault();
      stopped = true;
      inflight?.abort();
    };
    document.addEventListener('keydown', onKey);
    let sent = 0, received = 0, header = false;
    try {
      for (;;) {
        let payload = null, failure = null;
        const controller = new AbortController();
        inflight = controller;
        try { payload = await postWithSignal('/api/quick-connect/ping', { host, count: 1 }, controller.signal); }
        catch (error) { failure = error; }
        finally { inflight = null; }
        if (runId !== generation) return; // a newer command owns the scratchpad
        if (failure) {
          if (failure.name === 'AbortError') break;
          sent++;
          sink(`${sent}  ${failure.message || failure}\n`);
        } else {
          if (!header) { header = true; setStatus(`本机 ICMP → ${host} (${payload?.resolved || host})\n`); }
          sent++;
          if (payload?.ok) received++;
          sink(`${sent}  ${payload?.output || ''}\n`);
        }
        if (stopped) break;
        await new Promise(resolve => setTimeout(resolve, 1000));
        if (stopped || runId !== generation) break;
      }
    } finally {
      document.removeEventListener('keydown', onKey);
      inflight?.abort();
    }
    if (!header) setStatus(`本机 ICMP → ${host}\n`);
    sink(`收到 ${received} / ${sent} 个响应（已停止）\n\n`);
  }
  function parseTcping(command) {
    const tokens = command.trim().replace(/^tcping(?:\s+|$)/i, '').trim().split(/\s+/).filter(Boolean);
    let continuous = false;
    const targets = [];
    for (const token of tokens) {
      if (token === '-t' || token === '-T') continuous = true;
      else targets.push(token);
    }
    if (targets.length !== 2 || targets[0].startsWith('-')) return null;
    const port = Number(targets[1]);
    if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
    return { host: targets[0], port, continuous };
  }
  async function runTcping(command, sink, runId) {
    const target = parseTcping(command);
    if (!target) { sink('tcping 用法：tcping 主机 端口，可加 -t 连续拨号，按 Ctrl+C 停止。\n\n'); return; }
    if (runId !== generation) return;
    if (target.continuous) {
      const stopped = await runTcpingContinuous(sink, target.host, target.port, runId);
      if (runId !== generation) return;
      if (!stopped) sink('\n\n');
      return;
    }
    const pending = sink('正在 TCP 探测…\n');
    const write = text => { if (pending) pending(text); else sink(text); };
    await withInterrupt(async signal => {
      try {
        const payload = await postWithSignal('/api/local-probe/tcping', { host: target.host, port: target.port }, signal);
        write(`${(payload?.output || '').trimEnd()}\n\n`);
      } catch (error) {
        write(error?.name === 'AbortError' ? '^C 已停止。\n\n' : `${error.message || error}\n\n`);
      }
    });
  }
  // -t 模式复用 ping 连续探测的循环语义：每 500ms 一次 TCP 握手，Ctrl+C 停止并汇总。
  async function runTcpingContinuous(sink, host, port, runId) {
    const status = sink(`连续 TCP 拨号 ${host}:${port}，按 Ctrl+C 停止…\n`);
    const setStatus = text => { if (status) status(text); else sink(text); };
    let stopped = false, inflight = null, headerShown = false;
    const onKey = event => {
      if (!isInterruptKey(event) || window.getSelection()?.toString()) return;
      event.preventDefault();
      stopped = true;
      inflight?.abort();
    };
    document.addEventListener('keydown', onKey);
    let sent = 0, connected = 0;
    try {
      for (;;) {
        let payload = null, failure = null;
        const controller = new AbortController();
        inflight = controller;
        try { payload = await postWithSignal('/api/local-probe/tcping', { host, port, count: 1, brief: true }, controller.signal); }
        catch (error) { failure = error; }
        finally { inflight = null; }
        if (runId !== generation) return; // a newer command owns the scratchpad
        if (failure) {
          if (failure.name === 'AbortError') break;
          sent++;
          sink(`${sent}  ${failure.message || failure}\n`);
        } else {
          if (!headerShown) { headerShown = true; setStatus(`本机 TCP → ${host}:${port}\n`); }
          sent++;
          if (payload?.ok) connected++;
          sink(`${sent}  ${(payload?.output || '').split('\n').filter(Boolean).pop() || '无输出'}\n`);
        }
        if (stopped) break;
        await new Promise(resolve => setTimeout(resolve, 500));
        if (stopped || runId !== generation) break;
      }
    } finally {
      document.removeEventListener('keydown', onKey);
      inflight?.abort();
    }
    if (!headerShown) setStatus(`本机 TCP → ${host}:${port}\n`);
    sink(`已连接 ${connected} / ${sent} 次 TCP 拨号（已停止）\n\n`);
    return stopped;
  }
  function reflect() {
    if (!view?.isConnected) return;
    // The scratchpad belongs to the disconnected terminal; a live session owns
    // the area, and returning to no session starts from the welcome screen.
    if (current()) { generation++; view.textContent = ''; view.hidden = true; return; }
    if (view.textContent) { view.hidden = false; $('#welcome-state').hidden = true; }
    else { view.hidden = true; $('#welcome-state').hidden = false; }
  }
  function handleClear() {
    if (current() || !view?.isConnected || view.hidden || !view.textContent) return false;
    generation++;
    view.textContent = '';
    view.hidden = true;
    $('#welcome-state').hidden = false;
    return true;
  }
  function reset() { generation++; view?.remove(); view = null; }
  return { run, reflect, handleClear, termSink, commands, reset };
})();
