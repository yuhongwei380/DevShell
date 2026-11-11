'use strict';
window.DengQuickConnect = (() => {
  let dialog, form, output, saveDialog, savingID = '', saveGeneration = 0;
  const NEW_GROUP = '__new__';
  function open(command = '') {
    setDrawer(false);
    form.elements.command.value = command; output.textContent = '';
    const select = form.elements.keyId;
    select.replaceChildren(new Option('选择已导入的私钥', ''), ...managedKeys.map(k => new Option(k.name, k.id)));
    form.elements.auth.value = 'password'; select.parentElement.hidden = true;
    if (!dialog.open) dialog.showModal(); form.elements.command.focus();
  }
  async function run(command, auth = 'password', keyId = '') {
    if (/^ping(?:\s|$)/.test(command.trim())) {
      if (!dialog.open) open(command);
      output.textContent = '正在从本机探测…';
      try { output.textContent = (await post('/api/quick-connect/ping', { host: command.trim().slice(4).trim() })).output; }
      catch (error) { output.textContent = error.message; }
      return;
    }
    const profile = await post('/api/quick-connect', { command, auth, keyId });
    temporaryProfiles.set(profile.id, profile); dialog.close();
    if (profile.verbose) toast(`已开启 -${'v'.repeat(profile.verbose)} 连接诊断：终端显示调试信息，并写入 data/logs/ssh-disconnect.jsonl`);
    return connect(profile.id);
  }
  function canSave(state) {
    const profile = profileFor(state);
    return !!(state?.connected && profile?.temporary);
  }
  // 分组下拉列出服务器管理里的既有分组（子分组用 "A / B" 完整路径，与后端 legacyGroupPath 一致）；
  // 后端按名称匹配，选"新建分组…"并提交新名字时，不存在的名字会自动创建为顶层分组。
  function renderGroupChoices() {
    const select = saveDialog.querySelector('[name=group]'), extra = $('#quick-save-new-group');
    const tree = serverGroupTree(), names = new Set(['快速连接']);
    for (const group of tree.flat) { const path = tree.paths.get(group.id); if (path) names.add(path); }
    const existing = [...names].filter(name => name !== '快速连接').sort(compareServerNames);
    select.replaceChildren(new Option('快速连接', '快速连接'), ...existing.map(name => new Option(name, name)), new Option('＋ 新建分组…', NEW_GROUP));
    select.value = '快速连接';
    extra.hidden = true; saveDialog.querySelector('[name=groupNew]').value = '';
  }
  function saveCurrent(state = current()) {
    const profile = profileFor(state);
    if (!state?.connected || !profile?.temporary) return;
    savingID = profile.id; saveGeneration++;
    const f = saveDialog.querySelector('form'); f.elements.name.value = profile.name; renderGroupChoices();
    window.DengProfileNotes.init(f.elements.notes, profile.notes || '');
    // Quick-connect credentials the user just typed are remembered by default
    // when the connection is saved; the checkbox stays visible to opt out.
    const canRemember = profile.auth !== 'agent' && credentials.has(savingID);
    f.elements.remember.checked = canRemember; f.elements.remember.closest('label').hidden = !canRemember;
    // When nothing can be carried the checkbox disappears, which otherwise reads
    // like a saved credential; state plainly what the saved row will contain.
    $('#quick-save-note').textContent = profile.auth === 'agent' ? 'SSH Agent 认证无需保存凭据，之后由 Agent 提供。' : canRemember ? '勾选即把本次输入的凭据加密保存到本机，取消勾选则不保存。' : '本次没有可记住的凭据（这个窗口没有输入过密码），保存后下次连接仍需手动输入。';
    $('#quick-save-target').textContent = `${profile.user}@${profile.host}:${profile.port}`;
    $('#quick-save-error').textContent = ''; saveDialog.showModal(); f.elements.name.focus();
  }
  document.addEventListener('DOMContentLoaded', () => {
    dialog = node('dialog'); dialog.id = 'quick-connect-dialog'; dialog.setAttribute('aria-labelledby','quick-connect-title');
    dialog.innerHTML = `<form id="quick-connect-form"><div class="dialog-heading"><h2 id="quick-connect-title">快速连接</h2><button class="icon-button" type="button" aria-label="关闭快速连接" data-close><svg><use href="#i-close"/></svg></button></div>
      <label>SSH 地址或命令<input name="command" id="quick-connect-command" required maxlength="2048" autocomplete="off" spellcheck="false" placeholder="ssh -p 22 root@example.com"></label>
      <p>支持 用户@主机、ssh -p 端口 用户@主机、IPv6，或 ping 主机进行本机探测。连接成功后可保存到服务器列表。</p>
      <div class="form-row"><label>认证方式<select name="auth"><option value="password">密码</option><option value="key">私钥</option><option value="agent">SSH Agent</option></select></label><label hidden>私钥<select name="keyId"></select></label></div>
      <pre id="quick-connect-output" role="status"></pre><div class="dialog-buttons"><button type="button" class="upload-button" id="quick-open-manager">服务器管理</button><button type="submit" class="primary-button" id="quick-connect-submit">连接 / 探测</button></div></form>`;
    document.body.append(dialog); form = dialog.querySelector('form'); output = $('#quick-connect-output');
    dialog.querySelector('[data-close]').onclick = () => dialog.close();
    form.elements.auth.onchange = () => { form.elements.keyId.parentElement.hidden = form.elements.auth.value !== 'key'; };
    $('#quick-open-manager').onclick = () => { dialog.close(); setDrawer(true); };
    form.onsubmit = async e => {
      e.preventDefault(); const button = $('#quick-connect-submit'); if (button.disabled) return;
      button.disabled = true; output.textContent = '';
      try { await run(form.elements.command.value, form.elements.auth.value, form.elements.keyId.value); }
      catch (error) { output.textContent = error.message; }
      finally { button.disabled = false; }
    };
    $('#add-session').onclick = () => createNewSessionTab();
    const welcome = node('button', '', '快速连接'); welcome.id = 'welcome-quick-connect'; welcome.onclick = () => open(); $('#welcome-connect').before(welcome);
    saveDialog = node('dialog'); saveDialog.id = 'quick-save-dialog'; saveDialog.setAttribute('aria-labelledby','quick-save-title');
    saveDialog.innerHTML = `<form><div class="dialog-heading"><h2 id="quick-save-title">保存连接</h2><button type="button" class="icon-button" aria-label="取消保存" data-close><svg><use href="#i-close"/></svg></button></div><p id="quick-save-target"></p><label>名称<input name="name" required maxlength="100"></label><label>分组<select name="group"></select></label><label id="quick-save-new-group" hidden>新分组名称<input name="groupNew" maxlength="100" placeholder="输入新分组名称"></label><label>备注（可选）<textarea name="notes" rows="3" aria-describedby="quick-save-notes-hint" placeholder="续费网址、到期时间、用途等"></textarea><small id="quick-save-notes-hint" class="notes-input-hint" aria-live="polite">正文最多 3 行，每行最多 20 个中文或 40 个英文字符</small></label><label class="checkbox-label"><input name="remember" type="checkbox">记住本次登录凭据</label><p id="quick-save-note" role="status" class="notes-input-hint"></p><p id="quick-save-error" role="alert"></p><button type="submit" class="primary-button">保存连接</button></form>`;
    document.body.append(saveDialog); saveDialog.querySelector('[data-close]').onclick = () => saveDialog.close();
    saveDialog.querySelector('[name=group]').onchange = event => { const extra = $('#quick-save-new-group'); extra.hidden = event.target.value !== NEW_GROUP; if (!extra.hidden) saveDialog.querySelector('[name=groupNew]').focus(); };
    saveDialog.querySelector('form').onsubmit = async e => {
      e.preventDefault(); const f = e.currentTarget, b = f.querySelector('[type=submit]'), generation = saveGeneration; if (b.disabled || !window.DengProfileNotes.validate(f.elements.notes)) return;
      const choice = f.elements.group.value, group = choice === NEW_GROUP ? f.elements.groupNew.value.trim() : choice;
      if (!group) { $('#quick-save-error').textContent = '请填写新分组名称'; return; }
      b.disabled = true;
      try {
        const id = savingID, p = await post(`/api/quick-connect/${id}/save`, { name:f.elements.name.value,group,notes:f.elements.notes.value,remember:f.elements.remember.checked,secret:f.elements.remember.checked ? credentials.get(id) || '' : '' });
        temporaryProfiles.delete(id); profiles = profiles.filter(p => p.id !== id); profiles.push(p); renderTabs();
        if (generation === saveGeneration) saveDialog.close(); await loadProfiles(); toast('连接已保存');
      } catch (error) { if (generation === saveGeneration && saveDialog.open) $('#quick-save-error').textContent = error.message; } finally { b.disabled = false; }
    };
  }, { once:true });
  return { open, run, saveCurrent, canSave };
})();
