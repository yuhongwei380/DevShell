// Isolated browser fixture for issue #5: per-profile connect-time commands.
// Covers the Settings → 其他功能 master switch, the connection editor fields,
// persistence across reload, and the queue timing against shell integration
// signals without any real remote server.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const [stage,chrome,modulePath]=process.argv.slice(2);
const {default:puppeteer}=await import(modulePath);
const fixture=JSON.parse(fs.readFileSync(stage+'/browser-fixture.json'));
const origin=new URL(fixture.url).origin;
const setup=await fetch(origin+'/api/appearance/patch',{method:'POST',headers:{'Content-Type':'application/json','X-CloudShell-Token':fixture.token},body:JSON.stringify({onboardingCompleted:true,uiScale:1,theme:'dark'})});
assert(setup.ok);
const browser=await puppeteer.launch({executablePath:chrome,headless:true,args:['--no-sandbox','--disable-dev-shm-usage']});
const errors=[],checks=[];
try{
 const page=await browser.newPage();page.on('pageerror',e=>errors.push(e.message));
 await page.setViewport({width:1440,height:1000});
 await page.emulateMediaFeatures([{name:'prefers-reduced-motion',value:'reduce'}]);
 await page.goto(fixture.url,{waitUntil:'networkidle0'});
 await page.evaluate(()=>{document.querySelectorAll('dialog[open]').forEach(d=>d.close());pollStats=pollNetwork=pollLatency=()=>{};});

 // 1) The master switch lives under 设置 → 其他功能 and defaults to on.
 await page.click('#settings-button');
 await page.click('#manage-other-features');
 await page.waitForFunction(()=>!document.querySelector('#settings-other-view').hidden);
 assert.equal(await page.$eval('#feature-auto-commands',el=>el.value),'true','connect-command switch must default to 开启');
 assert.equal(await page.$eval('#feature-auto-commands',el=>el.selectedOptions[0].textContent),'开启');
 checks.push({menu:'other-features',default:'on'});
 await page.click('#other-settings-back');
 await page.evaluate(()=>setSettingsMenu(false));

 // 2) Connection editor (edit path, no auto-connect): blank lines never
 // reach the store and the summary counts the stored lines.
 const id=await page.evaluate(async()=>{const p=await post('/api/profiles',{name:'自动命令检查',host:'fixture.invalid',user:'root',port:22,auth:'password'});await loadProfiles();return p.id;});
 await page.evaluate(id=>showConnectionForm(profiles.find(p=>p.id===id)),id);
 await page.waitForSelector('#connection-dialog[open]');
 assert(await page.$eval('#auto-command-settings',el=>!el.hidden),'SSH profiles must show the command block');
 await page.$eval('#auto-command-settings',el=>{el.open=true;});
 await page.type('#connection-auto-commands','cd /data\n\n   \ntmux attach -t dev || tmux new -s dev\nuptime\n');
 await page.click('#connection-auto-run');
 assert.equal(await page.$eval('#auto-command-summary',el=>el.textContent.trim()),'已配置 3 条');
 await page.click('#save-connection');
 // The dialog closes before the refreshed list lands; wait on the data itself
 // instead of the dialog state (save is async by design).
 await page.waitForFunction(id=>!document.querySelector('#connection-dialog').open&&(profiles.find(p=>p.id===id)?.autoCommands||[]).length===3,{},id);
 let saved=await page.evaluate(id=>profiles.find(p=>p.id===id),id);
 assert.deepEqual(saved.autoCommands,['cd /data','tmux attach -t dev || tmux new -s dev','uptime'],'blank lines must not reach the store');
 assert.equal(saved.autoRun,true);

 // Switching the protocol hides the block (serial/RDP never run commands);
 // switching back re-fills the stored lines.
 await page.evaluate(id=>showConnectionForm(profiles.find(p=>p.id===id)),id);
 await page.select('#connection-protocol','serial');
 assert(await page.$eval('#auto-command-settings',el=>el.hidden),'serial profiles must hide the command block');
 await page.select('#connection-protocol','');
 assert(await page.$eval('#auto-command-settings',el=>!el.hidden));
 assert.equal(await page.$eval('#connection-auto-commands',el=>el.value),'cd /data\ntmux attach -t dev || tmux new -s dev\nuptime');
 await page.click('#cancel-connection');

 // 3) Editing without resubmitting keeps the stored lines (nil-merge contract).
 await page.evaluate(async id=>{await post('/api/profiles',{id,name:'自动命令检查',host:'fixture.invalid',user:'root',port:22,auth:'password'});await loadProfiles();},id);
 saved=await page.evaluate(id=>profiles.find(p=>p.id===id),id);
 assert.equal(saved.autoRun&&saved.autoCommands.length,3,'editing without the field must keep the commands');

 // 4) Runtime queue: prompt-gated typing, busy gating, global switch, disconnect.
 const runtime=await page.evaluate(id=>{
  const sent=[],profile=profiles.find(p=>p.id===id);
  const original=sendMessage;
  try{
   sendMessage=(state,message)=>{if(message&&message.type==='input')sent.push(message.data);};
   const s={...makeSessionState({id:'auto-runtime',profileId:id,home:'/'}),localOnly:true};
   sessions.set('auto-runtime',s);createTerminal(s);s.connected=true;s.ready=true;
   s.shellIntegration={shell:'bash',nonce:'fixture',ready:true,atPrompt:true};
   scheduleAutoCommands(s);
   if(sent.length)sent.push('!!scheduled-too-early');
   advanceAutoCommands(s); // at the prompt: first line goes out
   advanceAutoCommands(s); // sendInput set atPrompt=false: busy, must not type
   const busyHeld=sent.length===1;
   appearance.autoCommandsEnabled=false;
   s.shellIntegration.atPrompt=true;advanceAutoCommands(s); // master switch off: stop
   const switchHeld=sent.length===1;
   appearance.autoCommandsEnabled=true;
   advanceAutoCommands(s); // back on: second line
   markSessionDisconnected(s);
   s.ready=true;s.shellIntegration.atPrompt=true;advanceAutoCommands(s); // disconnected: stay quiet
   const typed=[...sent];
   // A second session keeps its own queue and never shares state.
   const t={...makeSessionState({id:'auto-runtime-2',profileId:id,home:'/'}),localOnly:true};
   sessions.set('auto-runtime-2',t);createTerminal(t);t.connected=true;t.ready=true;
   t.shellIntegration={shell:'bash',nonce:'fixture',ready:true,atPrompt:true};
   scheduleAutoCommands(t);advanceAutoCommands(t);
   return{typed,busyHeld,switchHeld,queueCleared:s.autoCommands===null,isolated:t.autoIndex===1&&sent.length===typed.length+1};
  }finally{sendMessage=original;}
 },id);
 assert.deepEqual(runtime.typed,['cd /data\r','tmux attach -t dev || tmux new -s dev\r'],`prompt-gated typing mismatch: ${JSON.stringify(runtime)}`);
 assert(runtime.busyHeld,'a busy integrated shell must not receive the next line');
 assert(runtime.switchHeld,'the master switch must stop the queue');
 assert(runtime.queueCleared,'disconnect must clear the queued commands');
 assert(runtime.isolated,'each session keeps its own queue');
 checks.push({typed:runtime.typed.length,busyHeld:runtime.busyHeld,switchHeld:runtime.switchHeld,isolated:runtime.isolated});

 // 5) Reload keeps the profile commands and the master switch on.
 await page.reload({waitUntil:'networkidle0'});
 await page.evaluate(()=>{document.querySelectorAll('dialog[open]').forEach(d=>d.close());pollStats=pollNetwork=pollLatency=()=>{};});
 const reloaded=await page.evaluate(id=>({enabled:appearance.autoCommandsEnabled,profile:profiles.find(p=>p.id===id)}),id);
 assert.equal(reloaded.enabled,true);
 assert.equal(reloaded.profile.autoCommands.length,3);
 assert.equal(reloaded.profile.autoRun,true);

 // 6) Turning the master switch off persists across reload.
 await page.click('#settings-button');await page.click('#manage-other-features');
 await page.waitForFunction(()=>!document.querySelector('#settings-other-view').hidden);
 await page.select('#feature-auto-commands','false');
 await page.evaluate(()=>appearanceSave);
 assert.equal(await page.evaluate(()=>appearance.autoCommandsEnabled),false);
 await page.reload({waitUntil:'networkidle0'});
 await page.evaluate(()=>{document.querySelectorAll('dialog[open]').forEach(d=>d.close());});
 assert.equal(await page.evaluate(()=>appearance.autoCommandsEnabled),false);

 assert.deepEqual(errors,[]);
 fs.writeFileSync(stage+'/auto-commands-result.json',JSON.stringify({passed:true,checks,errors},null,2));
 console.log('PASS: 其他功能 master switch default on and persists; editor block SSH-only with blank-line filtering; commands survive field-less edits; prompt-gated queue (busy held, switch off, disconnect clears, per-session isolation); reload preserved');
}finally{await browser.close();}
