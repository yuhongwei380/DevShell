// Render the pinned WebView assets against an isolated real Go configuration.
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const [fixturePath,modulePath,browserPath,output]=process.argv.slice(2);
const {default:puppeteer}=await import(pathToFileURL(path.resolve(modulePath)));
const fixture=JSON.parse(await fs.readFile(fixturePath,'utf8'));
await fs.mkdir(output,{recursive:true});
const browser=await puppeteer.launch({executablePath:browserPath,headless:true,args:['--no-sandbox']});
try {
 const page=await browser.newPage();await page.emulateMediaFeatures([{name:'prefers-reduced-motion',value:'reduce'}]);await page.setViewport({width:1280,height:800,deviceScaleFactor:1});
 await page.goto(fixture.url,{waitUntil:'networkidle0'});
 await page.evaluate(async()=>{await chooseAppearance({onboardingCompleted:true,theme:'light',uiScale:1,monitorVisible:true});});
 await page.reload({waitUntil:'networkidle0'});
 await page.evaluate(()=>{for(const dialog of document.querySelectorAll('dialog[open]'))dialog.close();document.querySelector('#window-controls').hidden=false;});
 await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 await page.evaluate(enrollment=>{const realFetch=window.fetch;window.fetch=(url,options)=>{
  const path=String(url),status=window.__captureSecurity;
  if(status&&path.endsWith('/api/security-lock/status'))return Promise.resolve(new Response(JSON.stringify(status),{headers:{'Content-Type':'application/json'}}));
  if(status&&path.endsWith('/api/security-lock/authorize'))return Promise.resolve(new Response(JSON.stringify({grant:'visual-only'}),{headers:{'Content-Type':'application/json'}}));
  if(status&&path.endsWith('/api/security-lock/enroll'))return Promise.resolve(new Response(JSON.stringify(enrollment),{headers:{'Content-Type':'application/json'}}));
  return realFetch(url,options);
 };},fixture.enrollment);
 const metrics=globalThis.captureMetrics={};
 const capture=async name=>{
  await new Promise(resolve=>setTimeout(resolve,250));
  await page.screenshot({path:path.join(output,name+'.png')});
  metrics[name]=await page.evaluate(()=>Object.fromEntries([...document.querySelectorAll('[id]')].filter(e=>e.getBoundingClientRect().width>0).map(e=>{
   const r=e.getBoundingClientRect(),s=getComputedStyle(e);return [e.id,{x:r.x,y:r.y,width:r.width,height:r.height,font:s.font,fontFamily:s.fontFamily,color:s.color,background:s.backgroundColor,radius:s.borderRadius,gap:s.gap}];
  })));
  metrics[name]._design=await page.evaluate(()=>Object.fromEntries(['body','.terminal-panel','#welcome-state h1','#welcome-state p','.server-manager-tabs','.server-folder-pane','.server-folder-row[data-level="1"]','.server-folder-selected','.server-profile-row','.connection-card','.connection-card-notes','.connection-search','.settings-caption','.scale-setting','.themed-select-trigger','#connection-form .dialog-heading','#connection-form .connection-form-body','#connection-form .connection-options','#connection-form .connection-form-footer'].map(selector=>{const e=document.querySelector(selector);if(!e)return[selector,null];const r=e.getBoundingClientRect(),s=getComputedStyle(e);return [selector,{box:[r.x,r.y,r.width,r.height],color:s.color,background:s.backgroundColor,gradient:s.backgroundImage,border:s.border,borderRadius:s.borderRadius,font:s.font,gap:s.gap,padding:s.padding,margin:s.margin}];})));
  metrics[name]._connection=await page.evaluate(()=>[...document.querySelectorAll('#connection-form input,#connection-form select,#connection-form textarea,#connection-form h3,#connection-form summary')].filter(e=>e.getBoundingClientRect().width>0).map(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return {element:e.name||e.id||e.textContent,box:[r.x,r.y,r.width,r.height],color:s.color,background:s.backgroundColor,font:s.font,padding:s.padding,border:s.border};}));
 };
 await page.evaluate(()=>DevShellTheme.set('light'));
 await capture('main-light');
 await page.evaluate(()=>setDrawer(true));await capture('servers-light');
 await page.click('#new-connection');await capture('connection-light');
 await page.click('#cancel-connection');await page.click('#close-connections');await page.click('#settings-button');await capture('settings-light');
 await page.evaluate(()=>{setSettingsMenu(false);setDrawer(false);DevShellTheme.set('dark');});await new Promise(resolve=>setTimeout(resolve,1000));
 await capture('main-dark');
 await page.evaluate(()=>setDrawer(true));await capture('servers-dark');
 await page.click('#new-connection');await capture('connection-dark');
 await page.click('#cancel-connection');await page.click('#close-connections');await page.click('#settings-button');await capture('settings-dark');
 for (const theme of ['light','dark']) {
  await page.evaluate(theme=>{setSettingsMenu(false);setDrawer(false);for(const d of document.querySelectorAll('dialog[open]'))d.close();DevShellTheme.set(theme);},theme);
  for (const [stage,button] of [['ui-style','manage-ui-styles'],['charts','manage-chart-styles'],['shell-fonts','manage-shell-fonts'],['ui-fonts','manage-ui-appearance'],['ansi','manage-ansi-schemes'],['prompt-colors','manage-prompt-colors'],['security','manage-security-lock'],['keys','manage-keys']]) {
   const found=await page.evaluate(button=>{const b=document.getElementById(button);if(!b)return false;b.click();return true;},button);
   if(!found)continue;
   await new Promise(resolve=>setTimeout(resolve,750));await capture(stage+'-'+theme);
   if(stage==='ansi'){
    await page.evaluate(()=>{document.getElementById('ansi-schemes-dialog').close();appearance.ansiSchemes=[{name:'柔和测试方案',colors:ansiPalettes.soft},{name:'鲜亮测试方案',colors:ansiPalettes.bright}];appearance.ansiPalette='custom';appearance.ansiScheme='柔和测试方案';document.getElementById('manage-ansi-schemes').click();});
    await capture('ansi-populated-'+theme);await page.evaluate(()=>{appearance.ansiSchemes=[];appearance.ansiPalette='soft';appearance.ansiScheme='';});
   }
   if(stage==='keys'){
    for(const [name,trigger] of [['key-import','import-key'],['key-generate','generate-key']]){await page.click('#'+trigger);await capture(name+'-'+theme);await page.click('#cancel-key-editor');}
   }
   if(stage==='security'){
    await page.evaluate(()=>{document.getElementById('security-lock-settings').close();window.__captureSecurity={enabled:true,locked:false,passwordConfigured:true,twoFactorConfigured:true,idleSeconds:300,revision:100};document.getElementById('manage-security-lock').click();});
    await new Promise(resolve=>setTimeout(resolve,200));await capture('security-authorize-'+theme);
    await page.type('#security-auth-value','fixture-proof');await page.click('#security-auth-button');await new Promise(resolve=>setTimeout(resolve,150));await capture('security-enabled-'+theme);
    await page.click('#security-enroll-start');await new Promise(resolve=>setTimeout(resolve,150));await capture('security-enrollment-'+theme);
    await page.evaluate(()=>{const body=document.querySelector('.security-settings-body');body.scrollTop=body.scrollHeight;});await capture('security-enrollment-bottom-'+theme);
    await page.evaluate(()=>{window.__captureSecurity={enabled:false,locked:false,passwordConfigured:false,twoFactorConfigured:false,revision:100};});
   }
   await page.evaluate(()=>{for(const d of document.querySelectorAll('dialog[open]'))d.close();});
  }
 }
 const filesFixture=JSON.parse(await fs.readFile('native-ui/assets/file-fixture.json','utf8'));
 await page.evaluate(data=>{
  const realApi=api;api=async(path,options)=>path.startsWith('/api/sessions/native-fixture/files')?{path:new URL('http://fixture'+path).searchParams.get('path'),entries:data.folders[new URL('http://fixture'+path).searchParams.get('path')]||[]}:path.startsWith('/api/sessions/native-fixture/file-permissions')?{mode:'0644',owner:'www / www'}:path==='/api/sessions/native-fixture/upload-check'?{conflicts:[{path:'/var/www/app.js',canOverwrite:true},{path:'/var/www/index.html',canOverwrite:true}]}:realApi(path,options);
  const state={name:'Dev Server',history:[],historyIndex:0,id:'native-fixture',profileId:profiles.find(p=>!p.protocol)?.id,connected:true,ready:true,cwd:data.path,entries:data.entries,sftpAvailable:true,transfers:[]};
  state.host=node('div','terminal-session');document.querySelector('#terminal-output').append(state.host);state.term=new Terminal({cols:120,rows:30,fontSize:14,fontFamily:terminalFontFamily,lineHeight:1.25,allowTransparency:true,theme:terminalTheme()});state.fit=new FitAddon.FitAddon();state.term.loadAddon(state.fit);state.term.open(state.host);state.term.write('root@dev-server:/var/www# ');sessions.set(state.id,state);activeID=state.id;renderTabs();renderSessionInfo();renderFiles();
 },filesFixture);
 await new Promise(resolve=>setTimeout(resolve,200));
 for(const path of ['/var','/var/www']){await page.evaluate(path=>document.querySelector('[data-path="'+path+'"] .tree-toggle')?.click(),path);await new Promise(resolve=>setTimeout(resolve,100));}
 for(const theme of ['light','dark']){
  await page.evaluate(theme=>{setSettingsMenu(false);setDrawer(false);for(const d of document.querySelectorAll('dialog[open]'))d.close();DevShellTheme.set(theme);},theme);
  for(const stage of ['files-bottom','files-top','files-right','files-selection','files-menu','files-single-menu','files-create-menu','files-open-with','files-new','files-delete','files-permissions','files-upload']){
   await page.evaluate(stage=>{appearance.filesPosition=stage==='files-right'?'right':stage==='files-top'?'top':'bottom';layout.files=300;appearance.monitorVisible=false;setWorkspaceVisible(true,false);showPane('files');applyWorkspacePlacement();applyUIScale();fileSelection(current()).names.clear();if(['files-selection','files-menu'].includes(stage)){fileSelection(current()).names.add('app.js');fileSelection(current()).names.add('index.html');}renderFiles();},stage);
   await new Promise(resolve=>setTimeout(resolve,500));
   if(stage==='files-menu')await page.evaluate(()=>DengFileTools.showMenu({clientX:650,clientY:545,preventDefault(){},stopPropagation(){}},current(),'/var/www/app.js',false,null,selectedFileTargets(current())));
   if(['files-single-menu','files-open-with','files-permissions'].includes(stage))await page.evaluate(stage=>{fileSelection(current()).names.add('app.js');renderFiles();DengFileTools.showMenu({clientX:650,clientY:300,preventDefault(){}},current(),'/var/www/app.js',false,null,selectedFileTargets(current()));if(stage==='files-open-with')document.querySelector('.file-submenu-wrap>button').click();if(stage==='files-permissions')[...document.querySelectorAll('.file-context-menu button')].find(b=>b.textContent==='文件权限').click();},stage);
   if(stage==='files-create-menu')await page.evaluate(()=>{const trigger=document.getElementById('create-file-menu');DengFileTools.showMenu({clientX:1040,clientY:565,preventDefault(){}},current(),null,false,trigger||document.querySelector('.file-toolbar button'));});
   if(stage==='files-new')await page.evaluate(()=>{void ask({title:'新建文件',description:'/var/www',input:true,confirm:'创建'});});
   if(stage==='files-delete')await page.evaluate(()=>{fileSelection(current()).names.add('app.js');fileSelection(current()).names.add('index.html');renderFiles();void DengFileTools.removeFiles(current(),selectedFileTargets(current()));});
   if(stage==='files-upload')await page.evaluate(()=>{void DengUploadConfirmation.prepare(current(),{directory:'/var/www',paths:[]});});
   await capture(stage+'-'+theme);await page.evaluate(()=>{document.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));if(document.getElementById('action-dialog').open)document.getElementById('action-cancel').click();document.querySelector('#upload-confirmation-dialog .icon-button')?.click();for(const d of document.querySelectorAll('dialog[open]'))d.close();});
  }
 }
 const monitorFixture=JSON.parse(await fs.readFile('native-ui/assets/monitor-fixture.json','utf8'));
 for(const theme of ['light','dark']){
  await page.evaluate(({theme,data})=>{const state=current();Date.now=()=>data.now;DevShellTheme.set(theme);appearance.monitorVisible=true;setWorkspaceVisible(false,false);state.stats=data.stats;state.networkStats=data.network;state.latency=data.latency;rememberNetworkSample(state,data.network);renderSessionInfo();applyUIScale();}, {theme,data:monitorFixture});
  await capture('monitor-'+theme);
 }
 // Inject only the security status response in this isolated reference page.
 // The original JS still builds and switches all lock-screen states.
 await page.evaluate(()=>{const realFetch=window.fetch;window.fetch=(url,options)=>String(url).endsWith('/api/security-lock/status')&&window.__captureLockStatus?Promise.resolve(new Response(JSON.stringify(window.__captureLockStatus),{headers:{'Content-Type':'application/json'}})):realFetch(url,options);});
 for(const theme of ['light','dark']){
  for(const stage of ['lock-message','lock-password','lock-totp','lock-retry','lock-exit']){
   await page.evaluate(({theme,stage})=>{
    DevShellTheme.set(theme);window.__captureLockStatus={enabled:true,locked:true,passwordConfigured:stage!=='lock-totp',twoFactorConfigured:true,passwordHint:'测试密码提示',revision:1000,retryAfterSeconds:stage==='lock-retry'?12:0};
    document.getElementById('security-unlock-form').hidden=true;document.getElementById('security-lock-message').hidden=false;document.getElementById('security-exit-form').hidden=true;
    DengSecurityLock.blocked();
   },{theme,stage});
   await new Promise(resolve=>setTimeout(resolve,200));
   if(stage!=='lock-message')await page.click('#security-unlock-open');
   await page.evaluate(stage=>{document.getElementById('security-window-close').hidden=false;if(stage==='lock-retry'){const method=document.getElementById('security-unlock-method');method.value='password';method.dispatchEvent(new Event('change'));}if(stage==='lock-exit')DengSecurityLock.confirmExit('确认关闭当前窗口。其他独立窗口会保持运行。',async()=>{},async()=>{});},stage);
   if(stage==='lock-retry')await page.evaluate(()=>{document.getElementById('security-unlock-error').textContent='密码或验证码不正确';});
   await capture(stage+'-'+theme);
   if(stage==='lock-exit')await page.click('#security-exit-cancel');
  }
 }
 await fs.writeFile(path.join(output,'metrics.json'),JSON.stringify(metrics,null,2));
 console.log('Pinned reference screenshots and metrics saved.');
}catch(error){console.error(String(error?.stack||error).split(fixture.token).join('[redacted]'));process.exitCode=1;}finally{if(globalThis.captureMetrics&&Object.keys(globalThis.captureMetrics).length)await fs.writeFile(path.join(output,'metrics.json'),JSON.stringify(globalThis.captureMetrics,null,2));await browser.close();}
