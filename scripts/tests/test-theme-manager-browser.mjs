// Run with Node 22+ and Edge/Chrome; no browser automation package required.
// Usage: node test-theme-manager-browser.mjs <isolated-fixture-dir> <browser-executable>
// Start TestFontLibraryBrowserFixture with DENG_FONT_QA_DIR first.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
const [stage, executable, previewURL] = process.argv.slice(2);
const fixture = JSON.parse(fs.readFileSync(path.join(stage, 'browser-fixture.json')));
fs.mkdirSync(path.join(stage, 'evidence'), {recursive:true});
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const headers = { 'Content-Type':'application/json', 'X-CloudShell-Token':fixture.token };
const setup = await fetch(new URL('/api/appearance/patch', fixture.url), { method:'POST', headers, body:JSON.stringify({onboardingCompleted:true, theme:'light', uiStyle:'classic', uiScale:1, backgroundId:'builtin:mist', terminalTransparency:.4, terminalFontSize:16, fontColors:{'builtin:jetbrains-mono':'#abcdef'}}) });
assert(setup.ok, await setup.text());
const profile = path.resolve(stage, 'edge-profile');
fs.rmSync(path.join(profile, 'DevToolsActivePort'), {force:true});
const browser = spawn(executable, ['--headless', '--disable-gpu', '--disable-background-networking', '--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio:'ignore' });
let socket, nextID = 0;
const pending = new Map(), errors = [], requests = [], checks = [];
async function until(fn, message, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try { if (await fn()) return; } catch(error) { if (!/navigated|context.*destroyed|Cannot find context/i.test(error.message)) throw error; }
    await pause(80);
  }
  throw new Error(`Timed out: ${message}`);
}
function send(method, params = {}) {
  const id = ++nextID;
  return new Promise((resolve, reject) => { pending.set(id, {resolve,reject}); socket.send(JSON.stringify({id,method,params})); });
}
async function evaluate(expression) {
  const result = await send('Runtime.evaluate', {expression, awaitPromise:true, returnByValue:true, replMode:true});
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.text + ': ' + result.exceptionDetails.exception?.description);
  return result.result?.value;
}
async function ready() {
  await until(() => evaluate("typeof appearance !== 'undefined' && activeFontID !== '' && activeBackgroundID !== '' && window.DengPortablePreferences?.ready && !document.querySelector('#startup-loading')"), 'application ready');
}
async function click(selector) {
  assert(await evaluate(`!!document.querySelector(${JSON.stringify(selector)})`), `missing ${selector}`);
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);
}
async function selected(style, mode) {
  await until(() => evaluate(`!document.querySelector('#theme-manager-dialog').hasAttribute('aria-busy') && appearance.uiStyle === '${style}' && DevShellTheme.preference === '${mode}'`), `${style}/${mode}`);
}
try {
  const portFile = path.join(profile, 'DevToolsActivePort');
  await until(() => fs.existsSync(portFile), 'browser debugging port');
  const port = fs.readFileSync(portFile,'utf8').split('\n')[0];
  const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  socket = new WebSocket(pages.find(page => page.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve,reject) => { socket.onopen=resolve; socket.onerror=reject; });
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id) { const request=pending.get(message.id); pending.delete(message.id); message.error ? request?.reject(new Error(message.error.message)) : request?.resolve(message.result); }
    if (message.method==='Runtime.exceptionThrown') errors.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
    if (message.method==='Network.requestWillBeSent') requests.push(message.params.request.url);
  };
  await send('Runtime.enable'); await send('Network.enable'); await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', {width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'},{name:'prefers-color-scheme',value:'light'}]});
  await send('Page.navigate',{url:fixture.url}); await ready();
  assert.equal(await evaluate('document.documentElement.dataset.uiStyle'), 'classic');
  assert(await evaluate("!!document.querySelector('#manage-backgrounds') && !!document.querySelector('#background-opacity') && !!document.querySelector('#follow-system-theme')"),'original appearance controls must remain');
  assert.equal(await evaluate("document.querySelector('#manage-themes').nextElementSibling.id"),'manage-backgrounds','theme configuration must precede the original background manager');
  const settingsProbe = `() => ['main','appearance','other'].map(view => ({view,controls:[...document.querySelector('#settings-' + view + '-view').querySelectorAll('button[role=menuitem],select,input[role=switch]')].map(el => ({id:el.id,tag:el.tagName,text:el.tagName==='BUTTON'?el.textContent.replace(/\\s+/g,' ').trim():'',options:el.tagName==='SELECT'?[...el.options].map(option=>[option.value,option.textContent]):[]}))}))`;
  const productSettings = await evaluate(`(${settingsProbe})()`);
  await evaluate(`document.querySelectorAll('dialog[open]').forEach(d=>d.close()); setDrawer(false); pollStats=pollNetwork=pollLatency=()=>{};
    const id='theme-qa'; profiles.push({id,name:'主题验证',host:'fixture.invalid',user:'qa',port:22});
    window.qaState={...makeSessionState({id,profileId:id,home:'/'}),localOnly:true}; sessions.set(id,qaState);createTerminal(qaState);activate(id);
    await setConciseMode(false);setWorkspaceVisible(true);
    window.qaTerm=qaState.term; qaTerm.options.cursorBlink=false;
    window.qaWrite=text=>new Promise(resolve=>qaTerm.write(text,resolve));
    window.qaSnapshot=()=>{const row=qaState.host.querySelector('.xterm-rows > div');return [...row.querySelectorAll(':scope > span')].map(span=>({text:span.textContent,fg:getComputedStyle(span).color,bg:getComputedStyle(span).backgroundColor}));};
    window.qaSample='\\x1b[38;2;0;0;255;48;2;0;0;0mBLUE 中文\\x1b[0m 常规颜色';
    qaTerm.resize(100,30); await qaWrite(qaSample); await new Promise(r=>setTimeout(r,100)); window.qaBefore=qaSnapshot();`);
  await evaluate(`window.qaLayout=()=>[...document.querySelectorAll('.app-shell,.sidebar,.system-section,.system-section h2,.specs,.cpu-model,.server-addresses,.metric,.network-grid,.network-card,.disk-io,.process-card,.process-table,.main-panel,.session-bar,.brand,.session-tab,.workspace,.terminal-panel,.terminal-meta,.terminal-output,.terminal-command-bar,.files-panel,.file-tabs,.file-toolbar,.file-content,.file-status')].map(el=>{const r=el.getBoundingClientRect(),s=getComputedStyle(el);return {node:el.id||el.classList[0],rect:[r.x,r.y,r.width,r.height],display:s.display,gap:s.gap,padding:s.padding,margin:s.margin,border:s.borderWidth,radius:s.borderRadius,columns:s.gridTemplateColumns,rows:s.gridTemplateRows}});
    window.qaNativeColors=()=>[document.documentElement,document.body,...document.querySelectorAll('.sidebar,.system-section,.metric,.meter,.network-card,.process-card,.session-bar,.terminal-panel,.terminal-meta,.terminal-command-bar,.files-panel')].map(el=>{const s=getComputedStyle(el);return [s.color,s.backgroundColor,s.backgroundImage,s.borderColor,s.boxShadow]});
    window.qaLayouts={};`);
  const nativeLayoutProbe=await evaluate('qaLayout.toString()');
  await click('#settings-button'); await click('#manage-appearance-config'); await click('#manage-themes');
  assert.equal(await evaluate("document.querySelector('#theme-manager-dialog').open"),true);
  assert.equal(await evaluate("document.activeElement.dataset.themeStyle"),'classic');
  for (const mode of ['light','dark','system']) {
    await click(`[data-theme-mode="${mode}"]`); await selected('classic',mode);
    const result = await evaluate(`({style:document.documentElement.dataset.uiStyle,mode:DevShellTheme.preference,
      image:getComputedStyle(document.querySelector('.terminal-panel'),'::before').backgroundImage,
      sameTerm:qaTerm===current().term,colors:JSON.stringify(qaBefore)===JSON.stringify(qaSnapshot()),font:appearance.fontColors[appearance.fontId],size:appearance.terminalFontSize,
      modeSelected:document.querySelector('[data-theme-mode="${mode}"]').getAttribute('aria-pressed')})`);
    assert.equal(result.style,'classic');assert.equal(result.mode,mode);assert.match(result.image,/mist\.png/,'classic must retain its original background');assert(result.sameTerm);assert(result.colors);assert.equal(result.font,'#abcdef');assert.equal(result.size,16);assert.equal(result.modeSelected,'true');
    await evaluate('qaLayouts[DevShellTheme.current]=qaLayout()');
    const classicColors=await evaluate('qaNativeColors()');
    await evaluate("document.querySelector('link[href*=\"theme-manager.css\"]').disabled=true");
    assert.deepEqual(await evaluate('qaNativeColors()'),classicColors,'the theme layer changed the original classic palette');
    await evaluate("document.querySelector('link[href*=\"theme-manager.css\"]').disabled=false");
    checks.push(result);
  }
  await click('#close-theme-manager');
  await click('#settings-button');await click('#manage-other-features');
  assert.equal(await evaluate("document.querySelector('#settings-other-view').hidden"),false);
  assert(await evaluate("!!document.querySelector('#feature-auto-commands') && !!document.querySelector('#feature-local-shell')"));
  await click('#other-settings-back');await click('#manage-appearance-config');await click('#manage-backgrounds');
  await until(()=>evaluate("document.querySelector('#appearance-dialog').open && document.querySelector('#asset-list').dataset.kind==='background'"),'original background manager');
  await click('[data-asset-id="builtin:black"] .asset-choice');
  await until(()=>evaluate("activeBackgroundID==='builtin:black'"),'solid black background');
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.terminal-panel')).backgroundColor"),'rgb(0, 0, 0)');
  await click('[data-asset-id="builtin:mist"] .asset-choice');
  await until(()=>evaluate("activeBackgroundID==='builtin:mist'"),'original wallpaper restored');
  await evaluate("const slider=document.querySelector('#background-opacity');slider.value='39';slider.dispatchEvent(new Event('input',{bubbles:true}));slider.dispatchEvent(new Event('change',{bubbles:true}));await appearanceSave");
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.terminal-panel'),'::before').opacity"),'0.39');
  await click('#close-appearance');await evaluate('DengThemeManager.open()');
  await click('[data-theme-style="frosted"]');await selected('frosted','system');
  assert.equal(await evaluate('appearance.backgroundId'),'builtin:mist');
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.terminal-panel'),'::before').backgroundImage"),'none','glass must not display the classic wallpaper');
  for (const mode of ['light','dark','system']) {
    await click(`[data-theme-mode="${mode}"]`); await selected('frosted',mode);
    const glass = await evaluate(`({style:document.documentElement.dataset.uiStyle,resolved:DevShellTheme.current,preference:DevShellTheme.preference,
      modesHidden:document.querySelector('.theme-mode').hidden,toggleHidden:document.querySelector('#theme-toggle').hidden,
      colors:JSON.stringify(qaBefore)===JSON.stringify(qaSnapshot()),sameTerm:current().term===qaTerm,
      brandLeft:document.querySelector('.brand').getBoundingClientRect().left,sidebarRight:document.querySelector('.sidebar').getBoundingClientRect().right,
      sidebar:getComputedStyle(document.querySelector('.sidebar')).backdropFilter,background:getComputedStyle(document.body).backgroundColor})`);
    assert.equal(glass.resolved,mode==='dark'?'dark':'light');assert.equal(glass.preference,mode);assert(!glass.modesHidden);assert(!glass.toggleHidden);assert(glass.colors);assert(glass.sameTerm);assert.equal(glass.background,mode==='dark'?'rgb(57, 54, 51)':'rgb(232, 230, 223)');assert.equal(glass.sidebar,'blur(32px) saturate(0.85)');assert(glass.brandLeft<glass.sidebarRight,'header material moved the fixed app controls into the session tabs');assert.deepEqual(await evaluate('qaLayout()'),await evaluate('qaLayouts[DevShellTheme.current]'),'glass changed native workspace geometry');checks.push(glass);
  }
  assert.deepEqual(await evaluate('qaLayout()'),await evaluate('qaLayouts[DevShellTheme.current]'),'glass changed native workspace geometry');
  for(const scheme of ['dark','light']) {
    await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'},{name:'prefers-color-scheme',value:scheme}]});
    await until(()=>evaluate(`DevShellTheme.current==='${scheme}'`),'glass follows system appearance');
    assert.equal(await evaluate('DevShellTheme.preference'),'system');
  }
  // A rejected write restores the selected card and all applied styles.
  await evaluate(`window.qaFetch=window.fetch;window.fetch=async(input,options)=>String(input).includes('/api/appearance/patch')?new Response(JSON.stringify({error:'fixture rejects save'}),{status:500,headers:{'Content-Type':'application/json'}}):qaFetch(input,options);`);
  await click('[data-theme-style="classic"]');
  await until(()=>evaluate("!document.querySelector('#theme-manager-dialog').hasAttribute('aria-busy')"),'failed save rollback');
  assert.equal(await evaluate('appearance.uiStyle'),'frosted');assert.equal(await evaluate('document.documentElement.dataset.uiStyle'),'frosted');
  await click('[data-theme-mode="dark"]');
  await until(()=>evaluate("!document.querySelector('#theme-manager-dialog').hasAttribute('aria-busy')"),'failed mode save rollback');
  assert.equal(await evaluate('DevShellTheme.preference'),'system');assert.equal(await evaluate('appearance.theme'),'system');
  await evaluate('window.fetch=qaFetch');
  await click('[data-theme-style="classic"]'); await selected('classic','system');
  assert.match(await evaluate("getComputedStyle(document.querySelector('.terminal-panel'),'::before').backgroundImage"),/mist\.png/,'returning to classic restores the saved background');
  assert.equal(await evaluate('appearance.backgroundOpacity'),.39);
  assert.equal(await evaluate('DevShellTheme.current'),'light','classic retains the system preference');
  await click('[data-theme-mode="light"]');await selected('classic','light');
  await click('[data-theme-style="frosted"]');await selected('frosted','light');
  assert.equal(await evaluate("document.querySelector('.theme-mode').hidden"),false);
  await evaluate("document.querySelector('#toast').hidden=true");
  await pause(100);
  for(const mode of ['light','dark']) {
    await click(`[data-theme-mode="${mode}"]`);await selected('frosted',mode);
    const managerShot=await send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(stage,`evidence/theme-manager-${mode}.png`),Buffer.from(managerShot.data,'base64'));
    await click('#close-theme-manager');await until(()=>evaluate("document.activeElement.id==='settings-button'"),'focus return');
    const workspaceShot=await send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(stage,`evidence/glass-workspace-${mode}.png`),Buffer.from(workspaceShot.data,'base64'));
    await evaluate('DengThemeManager.open()');
  }
  await click('[data-theme-mode="light"]');await selected('frosted','light');
  await evaluate('DengThemeManager.open()');
  await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'},{name:'prefers-reduced-transparency',value:'reduce'}]});
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.sidebar')).backdropFilter"),'none');
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.terminal-panel')).backgroundColor"),'rgb(36, 36, 35)');
  await send('Emulation.setEmulatedMedia',{features:[{name:'forced-colors',value:'active'}]});
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.sidebar')).backdropFilter"),'none');
  await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
  await click('#close-theme-manager');await until(()=>evaluate("document.activeElement.id==='settings-button'"),'focus return');

  await evaluate("await openAppearance('font')");
  assert.equal(await evaluate("document.querySelector('#appearance-dialog').dataset.kind"),'font');
  assert((await evaluate("document.querySelectorAll('#asset-list [data-font-id]').length"))>0,'font manager regressed');
  await evaluate("document.querySelector('#appearance-dialog').close();await DengUIAppearance.open()");
  assert.equal(await evaluate("document.querySelector('[data-ui-color-theme=light]').hidden"),false);
  assert.equal(await evaluate("document.querySelector('#ui-text-title').textContent"),'文字颜色 · 浅色 / 深色');
  await evaluate("document.querySelector('#ui-appearance-dialog').close();await chooseAppearance({uiStyle:'frosted'});await appearanceSave");
  const requestsBeforeReload = requests.length;
  await evaluate('window.qaReload=true');
  await send('Page.reload',{ignoreCache:true});
  await until(()=>evaluate('window.qaReload!==true'),'reload completed');await ready();
  assert.equal(await evaluate('document.documentElement.dataset.uiStyle'),'frosted');assert.equal(await evaluate('DevShellTheme.preference'),'light');assert.equal(await evaluate('DevShellTheme.current'),'light');
  assert.equal(await evaluate('appearance.backgroundId'),'builtin:mist','legacy background data must survive');
  assert.equal(requests.slice(requestsBeforeReload).some(url=>/\/assets\/backgrounds\/[^/?]+\.(png|jpe?g|webp)/.test(url)),false,'glass startup must not load classic wallpaper images');
  assert.deepEqual(await evaluate(`(${settingsProbe})()`),productSettings,'theme switching must retain all original settings controls');
  for(const width of [768,1100,1440]) {
    await send('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await pause(100);
    await evaluate('DengThemeManager.open()');
    assert(await evaluate("(()=>{const r=document.querySelector('#theme-manager-dialog').getBoundingClientRect();return r.left>=0&&r.right<=innerWidth&&r.top>=0&&r.bottom<=innerHeight})()"),`dialog overflows at ${width}`);
    await click('#close-theme-manager');
  }
  // Theme materials must preserve the native three SFTP placements, including
  // hidden/compact monitoring layouts, at the supported desktop widths.
  await evaluate(`window.qaLayout=(${nativeLayoutProbe});document.querySelector('#workspace').hidden=false`);
  const nativeLayoutChecks=[];
  for(const width of [1100,1440]) {
    await send('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});await pause(100);
    for(const position of ['right','bottom','top']) {
      await evaluate(`document.documentElement.dataset.filesPosition='${position}'`);
      for(const compact of [false,true]) {
        await evaluate(`document.documentElement.dataset.monitorVisible='${!compact}';document.documentElement.dataset.concise='${compact}';document.querySelector('#files-panel').hidden=document.querySelector('#files-splitter').hidden=${compact};document.querySelector('#workspace').classList.toggle('files-hidden',${compact})`);
        for(const mode of ['light','dark']) {
          await evaluate(`DevShellTheme.set('${mode}');DevShellTheme.setStyle('classic')`);await pause(80);
          const native=await evaluate('qaLayout()');
          await evaluate("DevShellTheme.setStyle('frosted')");
          assert.deepEqual(await evaluate('qaLayout()'),native,`native geometry changed: ${width}/${position}/${compact}/${mode}`);
          nativeLayoutChecks.push({width,position,compact,mode});
        }
      }
    }
  }
  await evaluate("document.documentElement.dataset.monitorVisible='true';document.documentElement.dataset.concise='false';document.documentElement.dataset.filesPosition='right';DevShellTheme.set('light')");
  if(previewURL) {
    await send('Emulation.setDeviceMetricsOverride',{width:1280,height:1000,deviceScaleFactor:1,mobile:false});
    await send('Page.navigate',{url:previewURL});
    await until(()=>evaluate("document.documentElement.dataset.designPreview==='true' && !!document.querySelector('#process-list tr')"),'native design preview');
    assert.equal(await evaluate("document.documentElement.dataset.mode"),'dark');
    assert.equal(await evaluate("document.querySelector('.sidebar').getBoundingClientRect().width"),304);
    assert.equal(await evaluate("document.querySelectorAll('.system-section > .metric').length"),3);
    assert.equal(await evaluate("document.querySelectorAll('#process-list tr').length"),5);
    assert(await evaluate("!!document.querySelector('#ping-status') && !!document.querySelector('#disk-filesystems .meter')"));
    assert.deepEqual(await evaluate(`(${settingsProbe})()`),productSettings,'preview must show the complete native settings menu');
    await click('#settings-button');
    assert.equal(await evaluate("document.querySelector('#settings-main-view').hidden"),false);
    await click('#manage-other-features');assert.equal(await evaluate("document.querySelector('#settings-other-view').hidden"),false);
    await click('#other-settings-back');await click('#manage-appearance-config');
    assert.equal(await evaluate("document.querySelector('#manage-themes').nextElementSibling.id"),'manage-backgrounds');
    await click('#manage-backgrounds');assert.equal(await evaluate("document.querySelector('#preview-setting-dialog').open"),true);
    await click('#close-preview-setting');await click('#settings-button');await click('#manage-appearance-config');await click('#manage-themes');
    for(const mode of ['light','dark']) {
      await click(`[data-theme-mode="${mode}"]`);
      assert.equal(await evaluate("document.documentElement.dataset.mode"),mode);
      await click('#close-theme-manager');
      const previewShot=await send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(stage,`evidence/glass-design-${mode}.png`),Buffer.from(previewShot.data,'base64'));
      await click('#settings-button');
      await click('#manage-appearance-config');await click('#manage-themes');
    }
    await click('[data-theme-style="classic"]');await click('[data-theme-mode="light"]');await click('[data-theme-style="frosted"]');
    assert.equal(await evaluate("document.documentElement.dataset.mode"),'light');
    await click('[data-theme-mode="system"]');
    await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-color-scheme',value:'dark'}]});
    await until(()=>evaluate("document.documentElement.dataset.mode==='dark'"),'preview follows system');


  }
  assert.deepEqual(errors,[]);
  fs.writeFileSync(path.join(stage,'theme-manager-results.json'),JSON.stringify({passed:true,checks,nativeLayoutChecks,errors,requests:requests.length},null,2));
  console.log(`PASS: ${checks.length} theme scenarios, native settings menu and original background manager, preview menu parity, glass light/dark/system, save rollback, restart restore, 24 native layouts, classic palette and background preservation, ANSI, fonts and responsive dialog.`);
} finally {
  if(socket?.readyState===WebSocket.OPEN) await send('Browser.close').catch(()=>{});
  socket?.close(); browser.kill();
}
