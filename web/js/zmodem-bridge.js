'use strict';
// lrzsz (`rz` / `sz`) speaks ZModem over the PTY, and this app has no ZModem
// peer. Instead of letting the remote program overstrike its startup string
// across the screen and then time out, bridge the intent to SFTP:
//   rz          -> pick local files and upload them to the terminal directory
//   sz <files>  -> download exactly those files through SFTP
// The half-open handshake is cancelled so lrzsz exits at once.
window.DengZmodemBridge = (() => {
 // Signatures taken from lrzsz 0.12.20 itself (captured from /usr/bin/rz and
 // /usr/bin/sz on a pty): the startup header is `**` + a CAN byte + 15 hex + CR,
 // sent twice, e.g. `**\x18B0100000023be50\r\x8a\x11`. `rz` first prints
 // "rz waiting to receive.", which names the direction on its own; other builds
 // overstrike the header with backspaces. A `**` + hex run with no CR only counts
 // when the shell just confirmed an `rz` / `sz` command.
 const header = /\*\*[\x00-\x20]{0,4}[0-9a-fA-F]{12,24}\r/;
 const overstrike = /\x08{2,}\*{0,2}[0-9a-fA-F]{12,24}/;
 const bare = /\*\*[\x00-\x20]{0,4}[0-9a-fA-F]{12,24}/;
 const receiving = /rz waiting to receive\./;
 // lrzsz repeats the advertisement until it dies. One attempt therefore gets one
 // SFTP flow and one note; later advertisements only keep poking the cancel
 // sequence. Re-opening the transfer here stacked native dialogs, and two modal
 // dialogs on one window snap the folder view back and hide the pointer.
 const intentTTL = 60000, attemptWindow = 30000, cancelGap = 250, cancelRounds = 3;
 const decoder = new TextDecoder('latin1');
 const intents = new Map(), streams = new Map(), cancellations = new Map();

 // Only the command the shell confirmed counts; a program merely printing
 // "sz" on screen never triggers this.
 function parse(command) {
  const line = String(command || '').trim();
  const match = /^(?:\/[^\s]*\/)?(rz|sz)(?=\s|$)/.exec(line);
  if (!match) return null;
  const kind = match[1], tokens = [];
  let current = '', quote = '';
  for (const character of line.slice(match[0].length)) {
   if (quote) { if (character === quote) quote = ''; else current += character; continue; }
   if (character === '"' || character === "'") { quote = character; continue; }
   if (/\s/.test(character)) { if (current) { tokens.push(current); current = ''; } continue; }
   current += character;
  }
  if (current) tokens.push(current);
  return { kind, files: kind === 'sz' ? pickNames(tokens) : [], at: Date.now() };
 }
 // Flags and "--" are lrzsz options, not file names.
 function pickNames(tokens) {
  const names = []; let options = true;
  for (const token of tokens) {
   if (token === '--') { options = false; continue; }
   if (options && token.startsWith('-')) continue;
   const name = token.replace(/\/+$/, '');
   if (name && name !== '.' && name !== '..' && !name.includes('\0')) names.push(token);
   if (names.length >= 20) break;
  }
  return names;
 }

 function noteCommand(state, command) {
  if (!state || state.serial) return;
  stopCancellation(state);
  const intent = parse(command);
  // A freshly confirmed rz / sz is a new attempt: neither the previous attempt's
  // window nor its half-read stream may carry over into it.
  if (intent) { intents.set(state.id, intent); state.zmodemBridgeUntil = 0; streams.delete(state.id); }
  else { intents.delete(state.id); streams.delete(state.id); }
 }
 function stopCancellation(state) {
  for (const timer of cancellations.get(state.id) || []) clearTimeout(timer);
  cancellations.delete(state.id);
 }
 function clear(state) { if (state) { stopCancellation(state); intents.delete(state.id); streams.delete(state.id); } }

 function sweep(now) { for (const [id, intent] of intents) if (now - intent.at > intentTTL) intents.delete(id); }

 // Cancel the half-open handshake the same way a receiving peer does. A shell
 // already sitting at a prompt means lrzsz is gone: never inject bytes then.
 // The first pair goes out at once — it is what actually stops lrzsz — and the
 // rest is paced on timers so the transfer never waits behind them; every later
 // advertisement pokes it again.
 function cancel(state) {
  if (state.closed || !state.connected || state.serial || state.shellIntegration?.atPrompt === true) return false;
  sendInput(state, '\x18\x18');
  return true;
 }
 function cancelHandshake(state) {
  stopCancellation(state);
  if (!cancel(state)) return;
  const timers = [];
  cancellations.set(state.id, timers);
  for (let round = 1; round < cancelRounds; round++) timers.push(setTimeout(() => {
   cancel(state);
   if (round === cancelRounds - 1) cancellations.delete(state.id);
  }, round * cancelGap));
 }

 async function act(state, intent) {
  const note = text => state.term.writeln(`\r\n\x1b[38;5;245mℹ️  ${text}\x1b[0m`);
  cancelHandshake(state);
  if (state.localOnly || state.serial) { note('DevShell 不支持 ZModem（lrzsz）传输，请使用 scp / sftp 传输文件。'); return; }
  if (state.sftpAvailable === false || !state.connected) { note('DevShell 不支持 ZModem（lrzsz）传输，而此连接的 SFTP 也不可用，请改用 scp / sftp。'); return; }
  const directory = state.terminalDirectory || state.cwd;
  if (intent?.kind === 'sz') {
   const names = intent.files;
   if (!names.length) { note('检测到 lrzsz 的 sz：DevShell 不支持 ZModem，请在下方 SFTP 面板下载对应文件。'); return; }
   note(`DevShell 不支持 ZModem（lrzsz），已改用 SFTP 下载：${names.join('、')}`);
   for (const name of names) { try { await DengFileTools.downloadFile(state, normalizePath(name, directory)); } catch (error) { toast(`下载 ${name} 失败：${error?.message || error}`); } }
   return;
  }
  if (intent?.kind === 'rz') { note(`DevShell 不支持 ZModem（lrzsz），已改用 SFTP 上传，目标目录：${directory}`); await chooseUploads(state, directory); return; }
  note('检测到 lrzsz（rz / sz）的 ZModem 握手：DevShell 不支持该协议，已中止。上传请使用下方 SFTP 面板，下载请使用文件右键菜单。');
 }

 function observe(state, bytes) {
  if (!state || state.serial || state.closed || !bytes?.length) return;
  const text = (streams.get(state.id) || '') + decoder.decode(bytes);
  // Keep a tail so a handshake split across output frames is still matched.
  streams.set(state.id, text.slice(-120));
  const now = Date.now(); sweep(now);
  // lrzsz's own "rz waiting to receive." names the direction even when the
  // shell integration is unavailable, so the upload bridge still works.
  const intent = intents.get(state.id) || (receiving.test(text) ? { kind: 'rz', files: [], at: now } : null);
  if (!(overstrike.test(text) || header.test(text) || (intent && bare.test(text)))) return;
  // A repeat advertisement of the attempt already handled: keep aborting it, but
  // never start a second transfer or print another note.
  if (now < (state.zmodemBridgeUntil || 0)) { cancel(state); return; }
  state.zmodemBridgeUntil = now + attemptWindow;
  intents.delete(state.id);
  act(state, intent).catch(error => toast(`传输失败：${error?.message || error}`));
 }

 return { parse, noteCommand, observe, clear };
})();
