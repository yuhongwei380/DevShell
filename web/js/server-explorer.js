/* Split server manager. UI state uses the portable encrypted preference store. */
'use strict';
function serverGroupColorText(color) {
  const channels = [1,3,5].map(i => parseInt(color.slice(i,i+2),16)/255).map(v => v <= .04045 ? v/12.92 : ((v+.055)/1.055)**2.4);
  const luminance = channels[0]*.2126 + channels[1]*.7152 + channels[2]*.0722;
  return luminance > .179 ? '#000000' : '#ffffff';
}
function saveServerExplorer() {
  save('dengshell.server-manager', { treeWidth: serverManager.treeWidth, includeChildren: serverManager.includeChildren, selectedGroup: serverManager.selectedGroup });
}
function chooseServerFolder(id, { toggle = false } = {}) {
  if (id && !serverManager.nodes.some(group => group.id === id)) return;
  if (toggle && id && id === serverManager.selectedGroup && !$('#connection-search').value.trim() && serverManager.nodes.some(group => group.parentId === id)) {
    toggleServerGroup(id); return;
  }
  serverManager.selectedGroup = id; serverManager.focusedProfile = ''; clearServerSelection();
  $('#connection-search').value = '';
  const byID = new Map(serverManager.nodes.map(group => [group.id, group])), seen = new Set();
  // Selecting a folder reveals its children and its ancestor path. Preserve
  // every other folder's collapsed state. Explicit navigation always reveals
  // the destination; activating the current row can toggle its children.
  for (let g = byID.get(id); g && !seen.has(g.id); g = byID.get(g.parentId)) { seen.add(g.id); serverManager.collapsed.delete(g.id); }
  persistGroupCollapse(); saveServerExplorer(); renderConnections(); $('#connection-groups').scrollTop = 0;
}
function serverExplorerProfiles(tree, query) {
  if (query) return profiles.filter(profile => serverMatches(profile, query, tree)).sort(compareServerNames);
  if (!serverManager.selectedGroup) return [...profiles].sort(compareServerNames);
  if (!serverManager.includeChildren) return [...(tree.members.get(serverManager.selectedGroup) || [])];
  const included = new Set([serverManager.selectedGroup]);
  for (const group of tree.flat) if (included.has(group.parentId)) included.add(group.id);
  return profiles.filter(profile => included.has(profile.groupId)).sort(compareServerNames);
}
function serverFolderAncestors(tree, id) {
  const chain = [], seen = new Set();
  for (let group = tree.byID.get(id); group && !seen.has(group.id); group = tree.byID.get(group.parentId)) { seen.add(group.id); chain.push(group); }
  return chain.reverse();
}
async function reorderServerGroup(id, targetID, before) {
  if (serverManager.groupOrderSaving || id === targetID) return;
  const tree = serverGroupTree(), group = tree.byID.get(id), target = tree.byID.get(targetID);
  if (!group || !target || (group.parentId || '') !== (target.parentId || '')) return;
  const siblings = tree.children.get(group.parentId || '') || [], current = siblings.map(item => item.id), ids = current.filter(item => item !== id);
  ids.splice(ids.indexOf(targetID) + (before ? 0 : 1), 0, id);
  if (ids.every((value, index) => value === current[index])) return;
  serverManager.groupOrderSaving = true;
  try {
    const groups = await post('/api/group-nodes/order', { parentId: group.parentId || '', ids });
    serverManager.generation++;
    serverManager.nodes = groups;
    renderConnections();
  } finally { serverManager.groupOrderSaving = false; }
}
function clearServerGroupDropHint() {
  for (const row of document.querySelectorAll('#server-folder-list .server-folder-row')) row.classList.remove('server-folder-drop-before', 'server-folder-drop-after');
}
function finishServerGroupDrag() {
  serverManager.groupDrag = null;
  clearServerGroupDropHint();
  for (const row of document.querySelectorAll('#server-folder-list .server-folder-dragging')) row.classList.remove('server-folder-dragging');
}
function enableServerGroupDrag(group, heading, select) {
  select.draggable = true;
  select.title += '\n拖动调整同级分组顺序；Alt + ↑ / ↓ 调整顺序';
  select.ondragstart = event => {
    if (serverManager.groupOrderSaving) { event.preventDefault(); return; }
    closeServerGroupMenu();
    serverManager.groupDrag = { id: group.id, parentId: group.parentId || '' };
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', group.id);
    heading.classList.add('server-folder-dragging');
  };
  select.ondragend = finishServerGroupDrag;
  const accepts = () => {
    const drag = serverManager.groupDrag;
    return drag && drag.id !== group.id && drag.parentId === (group.parentId || '') && !serverManager.groupOrderSaving;
  };
  heading.ondragover = event => {
    clearServerGroupDropHint();
    if (!accepts()) return;
    event.preventDefault(); event.dataTransfer.dropEffect = 'move';
    const bounds = heading.getBoundingClientRect(), before = event.clientY < bounds.top + bounds.height / 2;
    heading.classList.add(before ? 'server-folder-drop-before' : 'server-folder-drop-after');
    const folders = $('#server-folder-list'), pane = folders.getBoundingClientRect();
    if (event.clientY < pane.top + 24) folders.scrollTop -= 16;
    else if (event.clientY > pane.bottom - 24) folders.scrollTop += 16;
  };
  heading.ondragleave = event => { if (!heading.contains(event.relatedTarget)) clearServerGroupDropHint(); };
  heading.ondrop = safe(async event => {
    if (!accepts()) return;
    event.preventDefault(); event.stopPropagation();
    const id = serverManager.groupDrag.id, bounds = heading.getBoundingClientRect(), before = event.clientY < bounds.top + bounds.height / 2;
    finishServerGroupDrag();
    await reorderServerGroup(id, group.id, before);
  });
}
function renderServerExplorer(tree, query) {
  const folderPane = $('#server-folder-pane'), split = $('#server-manager-body'), folders = $('#server-folder-list');
  const serversTab = serverManager.tab === 'servers';
  split.classList.toggle('server-explorer-list-only', !serversTab);
  folderPane.hidden = !serversTab; $('#server-manager-divider').hidden = !serversTab; $('#server-explorer-heading').hidden = !serversTab;
  if (!serversTab) return;
  const left = folders.scrollLeft, top = folders.scrollTop;
  const fragment = document.createDocumentFragment(), ancestors = new Set(serverFolderAncestors(tree, serverManager.selectedGroup).map(group => group.id));
  const matches = new Set();
  if (query) {
    for (const group of tree.flat) if ((tree.paths.get(group.id) || '').toLocaleLowerCase().includes(query) || (tree.members.get(group.id) || []).some(profile => serverMatches(profile, query, tree))) {
      for (let g = group; g && !matches.has(g.id); g = tree.byID.get(g.parentId)) matches.add(g.id);
    }
  }
  function rowFor(group, depth, total, all = false) {
    const row = node('div', 'server-folder-wrap'); row.dataset.groupId = group.id; row.style.setProperty('--folder-depth', depth);
    const heading = node('div', 'server-folder-row'); heading.dataset.level = String(depth + 1); heading.classList.toggle('server-folder-selected', group.id === serverManager.selectedGroup); heading.classList.toggle('server-folder-ancestor', ancestors.has(group.id) && group.id !== serverManager.selectedGroup); if (all) heading.classList.add('server-folder-all');
    if (/^#[0-9a-f]{6}$/i.test(group.backgroundColor || '')) {
      heading.classList.add('server-folder-colored');
      heading.style.setProperty('--folder-color', group.backgroundColor);
      heading.style.setProperty('--folder-color-text', serverGroupColorText(group.backgroundColor));
    }
    const children = all ? [] : tree.children.get(group.id) || [], expanded = !!query || !serverManager.collapsed.has(group.id);
    const caret = node(children.length ? 'button' : 'span', 'server-folder-caret');
    if (children.length) {
      caret.type = 'button'; caret.dataset.groupAction = 'toggle'; caret.append(icon('chevron')); caret.setAttribute('aria-expanded', String(expanded)); caret.setAttribute('aria-label', `${expanded ? '折叠' : '展开'}分组 ${group.name}`); caret.onclick = () => toggleServerGroup(group.id);
    }
    const select = serverManagerButton('', () => {
      chooseServerFolder(group.id, { toggle: true });
      [...folders.querySelectorAll('[data-group-id]')].find(row => row.dataset.groupId === group.id)?.querySelector('[data-group-action="select"]')?.focus({ preventScroll: true });
    }, 'server-folder-select'); select.dataset.groupAction = 'select'; select.title = all ? '显示所有分组的连接' : tree.paths.get(group.id); select.setAttribute('aria-current', String(group.id === serverManager.selectedGroup));
    select.append(serverGroupIcon(group.emoji), node('strong', 'server-folder-name', group.name));
    select.append(node('span', 'server-folder-count', String(total)));
    select.setAttribute('aria-label', `${all ? '' : `${depth + 1}级目录，`}${group.name}，${total}个连接`);
    select.onkeydown = event => {
      if (!all && !query && event.altKey && ['ArrowUp', 'ArrowDown'].includes(event.key)) {
        event.preventDefault();
        const siblings = tree.children.get(group.parentId || '') || [], index = siblings.findIndex(item => item.id === group.id), before = event.key === 'ArrowUp';
        const target = siblings[index + (before ? -1 : 1)];
        if (target) safe(() => reorderServerGroup(group.id, target.id, before))();
        return;
      }
      const buttons = [...folders.querySelectorAll('[data-group-action="select"]')], index = buttons.indexOf(select);
      if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) { event.preventDefault(); buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : Math.max(0, Math.min(buttons.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))]?.focus(); }
      else if (!all && event.key === 'ArrowLeft') { event.preventDefault(); if (children.length && expanded) toggleServerGroup(group.id, true); else if (group.parentId) chooseServerFolder(group.parentId); }
      else if (children.length && event.key === 'ArrowRight') { event.preventDefault(); if (!expanded) toggleServerGroup(group.id, false); else chooseServerFolder(children[0].id); }
    };
    heading.append(caret, select);
    if (!all) {
      if (!query) enableServerGroupDrag(group, heading, select);
      const more = serverManagerButton('⋯', event => openServerGroupMenu(group.id, event.currentTarget), 'server-group-more'); more.dataset.groupAction = 'menu'; more.setAttribute('aria-label', `管理分组 ${group.name}`); more.setAttribute('aria-haspopup', 'menu'); heading.append(more);
      row.oncontextmenu = event => { event.preventDefault(); openServerGroupMenu(group.id, heading); };
    }
    row.append(heading); return row;
  }
  fragment.append(rowFor({ id: '', name: '全部连接' }, 0, profiles.length, true));
  let hiddenBelow = Infinity;
  for (const group of tree.flat) {
    const depth = tree.depths.get(group.id);
    if (depth <= hiddenBelow) hiddenBelow = Infinity;
    if (depth > hiddenBelow || query && !matches.has(group.id)) continue;
    fragment.append(rowFor(group, depth, tree.counts.get(group.id)));
    if (!query && serverManager.collapsed.has(group.id)) hiddenBelow = depth;
  }
  folders.replaceChildren(fragment); folders.scrollTop = top; folders.scrollLeft = left;
  const selected = tree.byID.get(serverManager.selectedGroup), chain = serverFolderAncestors(tree, serverManager.selectedGroup), items = serverExplorerProfiles(tree, query);
  $('#server-explorer-title').textContent = query ? '搜索结果' : selected?.name || '全部连接';
  $('#server-explorer-count').textContent = `${items.length} 个连接`;
  $('#server-include-children').checked = serverManager.includeChildren; $('#server-descendants-label').hidden = !!query || !selected;
  const path = $('#server-explorer-path'); path.replaceChildren();
  if (query) path.append(node('span', '', '在全部分组中搜索，点击左侧目录可返回分组浏览。'));
  else {
    const all = serverManagerButton('全部连接', () => chooseServerFolder(''), 'server-path-part'); path.append(all);
    const visible = chain.length > 7 ? [...chain.slice(0, 1), null, ...chain.slice(-5)] : chain;
    for (const group of visible) { path.append(node('span', 'server-path-separator', '›')); if (group) { const button = serverManagerButton(group.name, () => chooseServerFolder(group.id), 'server-path-part'); button.title = tree.paths.get(group.id); path.append(button); } else path.append(node('span', '', '…')); }
  }
  path.title = selected ? tree.paths.get(selected.id) : '全部分组';
  fitServerExplorer();
}
function reflectServerExplorerDetails() {
  const details = $('#server-explorer-details'); if (!details) return;
  const chosen = [...serverManager.selectedProfiles], profile = chosen.length === 1 ? profiles.find(p => p.id === chosen[0]) : null;
  details.replaceChildren();
  if (profile) {
    details.append(node('strong', '', `${profile.name} · ${profileAddressText(profile)}`));
    const tree = serverGroupTree(); details.append(node('span', '', `${serverGroupPath(profile, tree)}${profile.auth === 'key' && !profile.keyId && !profile.keyPath ? ' · 待配置私钥' : ''}`)); details.title = `${profile.name}\n${profileAddressText(profile)}\n${serverGroupPath(profile, tree)}`;
    if (profile.notes) {
      const note = serverManagerButton(`备注：${profile.notes.replace(/\s+/g, ' ')}`, () => window.DengProfileNotes.open(profile), 'server-notes-preview');
      note.title = '查看完整备注'; details.append(note);
    }
  } else { details.append(node('strong', '', chosen.length ? `已选择 ${chosen.length} 个连接` : '单击选择，双击打开连接'), node('span', '', '右键管理连接；可用 Ctrl / ⌘ 多选，再打开所选连接。')); details.removeAttribute('title'); }
}
function openServerProfileMenu(profile, actions, anchor) {
  closeServerGroupMenu(); const menu = $('#server-group-menu'); serverManager.menuProfile = profile.id;
  const buttons = [];
  if (!profile.deletedAt) buttons.push(serverManagerButton([...sessions.values()].some(s => s.profileId === profile.id && s.connected) ? profile.protocol === 'serial' ? '新建串口会话' : '新建 SSH 会话' : '连接', () => { closeServerGroupMenu(); return connect(profile.id); }));
  if (profile.notes) buttons.push(serverManagerButton('查看备注', () => window.DengProfileNotes.open(profile)));
  for (const button of actions.children) buttons.push(button);
  // 复制走后端 duplicate：名称与全部配置照抄，已保存的密码/私钥口令/代理口令
  // 在存储内随副本带走，明文不经过前端。
  if (!profile.deletedAt) buttons.push(serverManagerButton('复制到同名连接', async () => {
    closeServerGroupMenu();
    try {
      const saved = await post(`/api/profiles/${profile.id}/duplicate`);
      await loadProfiles();
      renderConnections();
      toast(`已复制为「${saved.name}」，已保存的凭据一并拷贝`);
    } catch (error) { toast(error.message); }
  }));
  if (!profile.deletedAt && window.DengPinnedNav) buttons.push(serverManagerButton(window.DengPinnedNav.isPinnedFor(profile) ? '从链接栏取消固定' : '固定到链接栏', async () => {
    closeServerGroupMenu();
    if (window.DengPinnedNav.isPinnedFor(profile)) await window.DengPinnedNav.unpinProfile(profile.id);
    else { await window.DengPinnedNav.pinProfile(profile.id); toast('已固定到链接栏'); }
  }));
  if (!profile.deletedAt) buttons.push(serverManagerButton('移动到…', () => openServerMoveDialog(profile)));
  if (!profile.deletedAt) buttons.push(serverManagerButton('定位所属分组', () => { closeServerGroupMenu(); serverManager.tab = 'servers'; chooseServerFolder(profile.groupId); serverManager.selectedProfiles.add(profile.id); reflectServerSelection(); const row = [...$('#server-folder-list').querySelectorAll('[data-group-id]')].find(row => row.dataset.groupId === profile.groupId); row?.scrollIntoView({ block: 'nearest', inline: 'nearest' }); }));
  buttons.push(serverManagerButton('复制地址', async () => { await copyText(profile.protocol === 'serial' ? profile.serialPort || profile.host : `${profile.user}@${profile.host}:${profile.port}`); closeServerGroupMenu(); toast('地址已复制'); }));
  // Preserve row action handlers when the same menu is reopened.
  menu.replaceChildren(...buttons.map(button => { const copy = button.cloneNode(true); copy.onclick = event => { closeServerGroupMenu(); return button.onclick?.(event); }; copy.setAttribute('role', 'menuitem'); return copy; }));
  menu.hidden = false;
  const scale = typeof effectiveScale === 'number' ? effectiveScale : 1, rect = anchor.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(rect.right / scale - menu.offsetWidth, innerWidth / scale - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(rect.bottom / scale + 4, innerHeight / scale - menu.offsetHeight - 8))}px`;
  menu.querySelector('button')?.focus();
}
function openServerMoveDialog(profile) {
  closeServerGroupMenu();
  const current = profiles.find(item => item.id === profile.id && !item.deletedAt);
  if (!current) return toast('连接不存在或已删除，请刷新后重试');
  const form = $('#server-move-form'); form.dataset.profileId = current.id;
  $('#server-move-profile').textContent = `将「${current.name}」移动到以下分组：`;
  $('#server-move-error').hidden = true;
  renderServerGroupChoices(current.groupId, $('#server-move-group'));
  $('#save-server-move').disabled = !serverManager.nodes.length;
  $('#server-move-dialog').showModal();
}
async function submitServerMove(event) {
  event.preventDefault();
  const button = $('#save-server-move'), select = $('#server-move-group'), cancel = $('#cancel-server-move');
  if (button.disabled) return;
  const id = event.currentTarget.dataset.profileId, groupId = select.value;
  const error = $('#server-move-error'); error.hidden = true;
  button.disabled = select.disabled = cancel.disabled = true;
  try {
    await post(`/api/profiles/${encodeURIComponent(id)}/group`, { groupId });
    await loadProfiles();
    $('#server-move-dialog').close();
    serverManager.tab = 'servers'; chooseServerFolder(groupId);
    serverManager.selectedProfiles.add(id); reflectServerSelection();
    const row = [...$('#connection-groups').querySelectorAll('[data-profile-id]')].find(item => item.dataset.profileId === id);
    row?.scrollIntoView({ block: 'nearest' }); row?.querySelector('.connection-card')?.focus({ preventScroll: true });
    toast('连接已移动到所选分组');
  } catch (failure) { error.textContent = failure.message || String(failure); error.hidden = false; }
  finally { button.disabled = select.disabled = cancel.disabled = false; }
}
function fitServerExplorer() {
  const drawer = $('#connections-drawer');
  if (drawer?.clientHeight) {
    drawer.classList.toggle('server-manager-compact', drawer.clientHeight < 620);
    drawer.classList.toggle('server-manager-narrow', drawer.clientWidth < 540);
  }
  const body = $('#server-manager-body'); if (!body?.clientWidth || serverManager.tab !== 'servers') return;
  const maximum = Math.max(145, Math.min(520, body.clientWidth - 195)), width = Math.max(145, Math.min(serverManager.treeWidth, maximum));
  body.style.setProperty('--server-tree-width', `${width}px`);
  const divider = $('#server-manager-divider'); divider.setAttribute('aria-valuenow', String(Math.round(width))); divider.setAttribute('aria-valuemax', String(Math.round(maximum)));
}
function initializeServerExplorer() {
  $('#cancel-server-move').onclick = () => $('#server-move-dialog').close();
  $('#server-move-dialog').oncancel = event => { if ($('#cancel-server-move').disabled) event.preventDefault(); };
  $('#server-move-form').onsubmit = safe(submitServerMove);
  const saved = readSaved('dengshell.server-manager', {});
  serverManager.treeWidth = Number.isFinite(saved?.treeWidth) ? Math.max(145, Math.min(520, saved.treeWidth)) : 320;
  serverManager.includeChildren = saved?.includeChildren !== false;
  if (typeof saved?.selectedGroup === 'string' && serverManager.nodes.some(g => g.id === saved.selectedGroup)) serverManager.selectedGroup = saved.selectedGroup;
  const body = node('div', 'server-manager-body'); body.id = 'server-manager-body';
  const folders = node('aside', 'server-folder-pane'); folders.id = 'server-folder-pane'; folders.setAttribute('aria-label', '分组目录');
  folders.innerHTML = '<div class="server-folder-heading"><strong>分组目录</strong><div><button type="button" id="server-expand-all" aria-label="展开全部分组" title="展开全部分组">展开</button><button type="button" id="server-collapse-all" aria-label="折叠全部分组" title="折叠全部分组">折叠</button></div></div><div id="server-folder-list" class="server-folder-list"></div>';
  const divider = node('div', 'server-manager-divider'); divider.id = 'server-manager-divider'; divider.tabIndex = 0; divider.setAttribute('role', 'separator'); divider.setAttribute('aria-orientation', 'vertical'); divider.setAttribute('aria-label', '调整分组栏宽度'); divider.setAttribute('aria-valuemin', '145'); divider.setAttribute('aria-controls', 'server-folder-pane'); divider.title = '拖动调整宽度；方向键调整，双击恢复';
  const pane = node('div', 'server-explorer-content'); pane.innerHTML = '<div id="server-explorer-heading"><div class="server-explorer-heading-row"><strong id="server-explorer-title"></strong><span id="server-explorer-count"></span></div><nav id="server-explorer-path" aria-label="当前分组路径"></nav><label id="server-descendants-label"><input type="checkbox" id="server-include-children">包含子分组</label></div>';
  const list = $('#connection-groups'); list.before(body); pane.append(list);
  const detail = node('div', 'server-explorer-details'); detail.id = 'server-explorer-details'; detail.setAttribute('role', 'status'); pane.append(detail); body.append(folders, divider, pane);
  $('#server-include-children').onchange = event => { serverManager.includeChildren = event.target.checked; clearServerSelection(); saveServerExplorer(); renderConnections(); };
  $('#server-expand-all').onclick = () => { serverManager.collapsed.clear(); persistGroupCollapse(); renderConnections(); };
  $('#server-collapse-all').onclick = () => { serverManager.collapsed = new Set(serverManager.nodes.map(g => g.id)); persistGroupCollapse(); chooseServerFolder(''); };
  folders.addEventListener('scroll', closeServerGroupMenu, { capture: true, passive: true });
  let drag = null;
  divider.onpointerdown = event => { if (event.button !== 0) return; const scale = typeof effectiveScale === 'number' ? effectiveScale : 1; drag = { x: event.clientX, width: folders.offsetWidth, scale }; divider.setPointerCapture(event.pointerId); event.preventDefault(); };
  divider.onpointermove = event => { if (!drag) return; serverManager.treeWidth = Math.max(145, Math.min(520, drag.width + (event.clientX - drag.x) / drag.scale)); fitServerExplorer(); };
  const finish = () => { if (drag) { drag = null; saveServerExplorer(); } };
  divider.onpointerup = finish; divider.onpointercancel = finish; divider.onlostpointercapture = finish;
  divider.onkeydown = event => { if (!['ArrowLeft', 'ArrowRight', 'Home'].includes(event.key)) return; event.preventDefault(); serverManager.treeWidth = event.key === 'Home' ? 320 : Math.max(145, Math.min(520, folders.offsetWidth + (event.key === 'ArrowRight' ? 20 : -20))); fitServerExplorer(); saveServerExplorer(); };
  divider.ondblclick = () => { serverManager.treeWidth = 320; fitServerExplorer(); saveServerExplorer(); };
  new ResizeObserver(fitServerExplorer).observe(body);
}
