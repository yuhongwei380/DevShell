import fs from 'node:fs';
import assert from 'node:assert/strict';
const [stage,chrome,modulePath]=process.argv.slice(2);
const {default:puppeteer}=await import(modulePath),fixture=JSON.parse(fs.readFileSync(stage+'/browser-fixture.json'));
await fetch(new URL(fixture.url).origin+'/api/appearance/patch',{method:'POST',headers:{'Content-Type':'application/json','X-CloudShell-Token':fixture.token},body:JSON.stringify({onboardingCompleted:true,uiScale:1})});
const browser=await puppeteer.launch({executablePath:chrome,headless:true,args:['--no-sandbox','--disable-dev-shm-usage']});
try {
 const page=await browser.newPage();await page.setViewport({width:1440,height:1000});await page.goto(fixture.url,{waitUntil:'networkidle0'});
 await page.evaluate(()=>{
  document.querySelectorAll('dialog[open]').forEach(d=>d.close());pollStats=()=>{};pollNetwork=()=>{};loadProfiles=async()=>{};
  for(const id of ['save-a','save-b']){
   const p={id,name:id,host:'example.invalid',user:'qa',port:22,temporary:true,auth:'password'};temporaryProfiles.set(id,p);
   sessions.set(id,{...makeSessionState({id,profileId:id,home:'/'}),localOnly:true,term:{options:{},focus(){},refresh(){},dispose(){}},host:document.createElement('div')});
  }
  const original=api;api=(url,body)=>url.endsWith('/save')?new Promise((resolve,reject)=>{window.qaSave={resolve,reject,url,body}}):original(url,body);
 });
 for(const outcome of ['success','failure']){
  await page.evaluate(()=>{activate('save-a');window.DengQuickConnect.saveCurrent();document.querySelector('#quick-save-dialog form').requestSubmit()});
  await page.waitForFunction(()=>!!window.qaSave);
  await page.evaluate(()=>{document.querySelector('#quick-save-dialog').close();activate('save-b');window.DengQuickConnect.saveCurrent()});
  await page.evaluate(outcome=>{if(outcome==='success')qaSave.resolve({...temporaryProfiles.get('save-a'),temporary:false});else qaSave.reject(Error('old request failed'));},outcome);
  await page.waitForFunction(()=>!document.querySelector('#quick-save-dialog [type=submit]').disabled);
  assert(await page.$eval('#quick-save-dialog',e=>e.open),'late save result must not close the newer dialog');
  assert.equal(await page.$eval('#quick-save-dialog [name=name]',e=>e.value),'save-b');
  assert.equal(await page.$eval('#quick-save-error',e=>e.textContent),'');
  await page.evaluate(()=>{document.querySelector('#quick-save-dialog').close();temporaryProfiles.set('save-a',{id:'save-a',name:'save-a',host:'example.invalid',user:'qa',port:22,temporary:true,auth:'password'});window.qaSave=null});
 }
 // Credentials typed during quick connect are remembered by default when the
 // connection is saved; without a cached credential the box stays hidden.
 await page.evaluate(()=>{credentials.set('save-a','qa-secret-pw');profiles=profiles.filter(p=>p.id!=='save-a');activate('save-a');window.DengQuickConnect.saveCurrent()});
 assert.equal(await page.$eval('#quick-save-dialog [name=remember]',e=>e.checked),true);
 assert.equal(await page.$eval('#quick-save-dialog [name=remember]',e=>e.closest('label').hidden),false);
 await page.evaluate(()=>{document.querySelector('#quick-save-dialog form').requestSubmit()});
 await page.waitForFunction(()=>!!window.qaSave);
 const saveBody=JSON.parse(await page.evaluate(()=>qaSave.body.body));
 assert.equal(saveBody.remember,true);assert.equal(saveBody.secret,'qa-secret-pw');
 await page.evaluate(()=>{qaSave.resolve({...temporaryProfiles.get('save-a'),temporary:false});document.querySelector('#quick-save-dialog').close();temporaryProfiles.set('save-b',{id:'save-b',name:'save-b',host:'example.invalid',user:'qa',port:22,temporary:true,auth:'password'});activate('save-b');window.DengQuickConnect.saveCurrent()});
 assert.equal(await page.$eval('#quick-save-dialog [name=remember]',e=>e.closest('label').hidden),true);
 assert.equal(await page.$eval('#quick-save-dialog [name=remember]',e=>e.checked),false);
 await page.evaluate(()=>{document.querySelector('#quick-save-dialog').close()});
 // The save dialog's group picker defaults to 快速连接, lists existing groups by
 // full path, and "＋ 新建分组…" submits the typed name (backend creates the group).
 await page.evaluate(()=>{window.qaSave=null;window.DengQuickConnect.saveCurrent()});
 assert.equal(await page.$eval('#quick-save-dialog [name=group]',e=>e.value),'快速连接');
 assert.equal(await page.$eval('#quick-save-new-group',e=>e.hidden),true);
 const groupValues=await page.$$eval('#quick-save-dialog [name=group] option',es=>es.map(e=>e.value));
 assert.equal(groupValues[0],'快速连接');assert.equal(groupValues.at(-1),'__new__');
 await page.evaluate(()=>{window.qaSave=null;const s=document.querySelector('#quick-save-dialog [name=group]');s.value='__new__';s.dispatchEvent(new Event('change'))});
 assert.equal(await page.$eval('#quick-save-new-group',e=>e.hidden),false);
 await page.evaluate(()=>{document.querySelector('#quick-save-dialog form').requestSubmit()});
 await new Promise(resolve=>setTimeout(resolve,100));
 assert.equal(await page.$eval('#quick-save-error',e=>e.textContent),'请填写新分组名称');
 assert.equal(await page.evaluate(()=>window.qaSave),null);
 await page.evaluate(()=>{document.querySelector('#quick-save-dialog [name=groupNew]').value='QA 新分组';document.querySelector('#quick-save-dialog form').requestSubmit()});
 await page.waitForFunction(()=>!!window.qaSave);
 assert.equal(JSON.parse(await page.evaluate(()=>qaSave.body.body)).group,'QA 新分组');
 assert.equal(JSON.parse(await page.evaluate(()=>qaSave.body.body)).name,'save-b');
 await page.evaluate(()=>{qaSave.resolve({...temporaryProfiles.get('save-b'),temporary:false});document.querySelector('#quick-save-dialog').close()});
 fs.writeFileSync(stage+'/quick-connect-races.json',JSON.stringify({passed:true,lateSaveSuccessIsolated:true,lateSaveFailureIsolated:true,quickCredentialsRememberedByDefault:true,groupPickerDefaultsNewGroupFlow:true}));console.log('PASS quick connect save races + credentials remembered by default + group picker new-group flow');
}finally{await browser.close()}
