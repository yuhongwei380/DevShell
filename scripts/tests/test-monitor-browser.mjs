import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import fs from 'node:fs';
import {join,isAbsolute} from 'node:path';
import {pathToFileURL} from 'node:url';
import {spawn} from 'node:child_process';
const [stage,chrome,modulePath]=process.argv.slice(2);
const {default:puppeteer}=await import(isAbsolute(modulePath)?pathToFileURL(modulePath).href:modulePath);
const fixture=JSON.parse(await readFile(join(stage,'browser-fixture.json'),'utf8'));
await fetch(new URL('/api/appearance/patch',fixture.url),{method:'POST',headers:{'Content-Type':'application/json','X-CloudShell-Token':fixture.token},body:JSON.stringify({onboardingCompleted:true,uiScale:1})});
const profileDir=fs.mkdtempSync(join(stage,'edge-monitor-'));
const child=spawn(chrome,['--headless=new','--no-sandbox','--disable-dev-shm-usage','--disable-background-networking','--remote-debugging-port=0','--user-data-dir='+profileDir,'about:blank'],{stdio:'ignore',windowsHide:true});
let browser;
try {
 let endpoint;
 for(let i=0;i<100;i++){try{const [port,suffix]=fs.readFileSync(join(profileDir,'DevToolsActivePort'),'utf8').trim().split(/\r?\n/);endpoint=`ws://127.0.0.1:${port}${suffix}`;break;}catch{}await new Promise(resolve=>setTimeout(resolve,100));}
 assert(endpoint,'browser started');
 browser=await puppeteer.connect({browserWSEndpoint:endpoint});
 const page=await browser.newPage();await page.setViewport({width:1440,height:1000});await page.emulateMediaFeatures([{name:'prefers-reduced-motion',value:'reduce'}]);const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(fixture.url,{waitUntil:'networkidle0'});
 const result=await page.evaluate(()=>{
  const original=activeID, state={connected:true,stats:{processSample:{available:true,sampledAt:new Date().toISOString(),intervalMilliseconds:5000},processes:Array.from({length:20},(_,i)=>({pid:i+1,name:'process-'+(i+1),cpu:i+1,cpuReady:true,memory:1024*(21-i),memoryReady:true,memoryEstimated:true})),processMemoryTop:Array.from({length:5},(_,i)=>({pid:100+i,name:'memory-'+i,cpu:0,cpuReady:true,memory:1024*(100-i),memoryReady:true,memoryEstimated:true}))},processSort:{key:'cpu',ascending:true}};
  sessions.set('monitor-fixture',state); activeID='monitor-fixture';
  const rows=()=>[...document.querySelectorAll('#process-list tr')].map(r=>Number(r.dataset.pid));
  try {
   renderProcesses(state.stats);const cpu=rows();sortProcesses('memory');const memory=rows();
   state.stats={processSample:{available:false,error:'fixture timeout'}};renderProcesses(state.stats);const cached=rows(),cacheLabel=document.querySelector('#process-count').textContent;
   return {cpu,memory,cached,cacheLabel,alwaysVisible:document.querySelector('.process-details').tagName==='SECTION'&&document.querySelector('.process-table').getBoundingClientRect().height>0,memoryCells:[...document.querySelectorAll('#process-list tr')].map(row=>row.cells[0].textContent),trafficWindow:trafficWindowMilliseconds};
  } finally {activeID=original; sessions.delete('monitor-fixture');}
 });
 assert.deepEqual(result.cpu,[20,19,18,17,16]);assert.deepEqual(result.memory,[100,101,102,103,104]);assert.deepEqual(result.cached,result.memory);assert.match(result.cacheLabel,/保留缓存/);assert(result.alwaysVisible);assert.deepEqual(result.memoryCells,Array(5).fill('—'),'estimated memory must not be presented as exact RSS');assert.equal(result.trafficWindow,60000);
 await (await page.$('.process-details')).screenshot({path:join(stage,'process-top-five.png')});
 // Check large mount points, the strict 10 GB boundary, tab changes and I/O
 // independently of capacity filtering, using the real page in both themes.
 const diskFixture=await readFile(join(stage,'disk-fixture.json'),'utf8').then(JSON.parse).catch(error=>{if(error.code==='ENOENT')return {cores:4,cpu:0.6,memoryUsed:3.9*1024**3,memoryTotal:7.8*1024**3,swapUsed:0,swapTotal:0,uptime:90000,disks:[{path:'/',used:1.5*1024**4,total:1.7*1024**4,available:0.2*1024**4},{path:'/data',used:1.4*1024**4,total:2.9*1024**4,available:1.5*1024**4},{path:'/boot/efi',used:6.2*1024**2,total:1.1*1024**3,available:1.09*1024**3}]};throw error;});
 const diskChecks=[];
 if(diskFixture){
  await page.evaluate(()=>{document.querySelectorAll('dialog[open]').forEach(d=>d.close());setDrawer(false);});
  for(const theme of ['dark','light']){
   const check=await page.evaluate(({stats,theme})=>{
    DevShellTheme.set(theme);
    stats.sampleReady=true;stats.diskRead=326.3*1024;stats.diskWrite=9.2*1024**2;
    const text=id=>document.querySelector('#disk-'+id).textContent;
    const filesystems=document.querySelector('#disk-filesystems');
    const rows=()=>[...filesystems.children].map(row=>({path:row.querySelector('span').textContent,capacity:row.querySelector('em').textContent,percent:row.querySelector('b').textContent,tooltip:row.title}));
    renderMonitor(stats);
    const io=document.querySelector('.disk-io'),network=document.querySelector('.traffic-card');
    const rates=[text('read'),text('write')];
    const filesystemRows=rows(),expectedRows=stats.disks.filter(disk=>disk.total>10*1024**3).map(disk=>({path:disk.path,capacity:`${prettySize(disk.used)} / ${prettySize(disk.total)}`,percent:`${(disk.used/disk.total*100).toFixed(1)}%`}));
    const layout={summaryAbsent:!document.querySelector('#disk-meter'),lastResource:filesystems.nextElementSibling.className==='network-grid',ioAboveNetwork:io.nextElementSibling===network,matchedStyle:getComputedStyle(filesystems.querySelector('.meter')).height===getComputedStyle(document.querySelector('#cpu-meter')).height,filesystemsVisible:!filesystems.hidden&&filesystems.getBoundingClientRect().height>0};
    renderMonitor({...stats,disks:[{path:'/',used:93*1024**3,total:100*1024**3,available:7*1024**3},{path:'/data',used:1500*1024**3,total:3000*1024**3,available:1500*1024**3}]});
    const nearlyFullRoot=rows();
    renderMonitor({...stats,disks:[{path:'/',used:1024**3,total:11*1024**3,available:10*1024**3}]});
    const singleRows=rows(),singleHidden=filesystems.hidden;
    renderMonitor(stats);renderMonitor(null);const emptyRates=[text('read'),text('write')],emptyRows=rows(),emptyHidden=filesystems.hidden;
    const limit=10*1024**3;
    renderMonitor({...stats,disks:[{path:'/below',used:1,total:limit-1,available:limit-2},{path:'/exact',used:1,total:limit,available:limit-1},{path:'/above',used:1,total:limit+1,available:limit}]});
    const boundaryRows=rows();
    renderMonitor({...stats,disks:[{path:'/',used:1024**3,total:limit,available:9*1024**3},{path:'/boot/efi',used:6*1024**2,total:1024**3,available:1018*1024**2},{path:'/zero',used:0,total:0,available:0}]});
    const smallRows=rows(),smallHidden=filesystems.hidden,smallRates=[text('read'),text('write')];
    renderMonitor({...stats,disks:[]});
    const noDiskRows=rows(),noDiskHidden=filesystems.hidden;
    renderMonitor(stats);
    return {theme,rates,filesystemRows,expectedRows,nearlyFullRoot,singleRows,singleHidden,emptyRates,emptyRows,emptyHidden,boundaryRows,smallRows,smallHidden,smallRates,noDiskRows,noDiskHidden,layout};
   },{stats:diskFixture,theme});
   assert.deepEqual(check.filesystemRows.map(({tooltip,...row})=>row),check.expectedRows);
   assert.equal(check.filesystemRows.length,2);assert(!check.filesystemRows.some(row=>row.path==='/boot/efi'));
   for(const row of check.filesystemRows){assert(row.tooltip.includes(row.path));assert.match(row.tooltip,/可用/);}
   assert.deepEqual(check.nearlyFullRoot.map(({path,percent})=>({path,percent})),[{path:'/',percent:'93.0%'},{path:'/data',percent:'50.0%'}]);
   assert.deepEqual(check.rates,['326.3 KB/s','9.2 MB/s']);assert.deepEqual(check.smallRates,check.rates);assert.deepEqual(check.emptyRates,['—','—']);
   assert.equal(check.singleRows.length,1);assert.equal(check.singleRows[0].path,'/');assert.equal(check.singleRows[0].capacity,'1.0 GB / 11.0 GB');assert(!check.singleHidden);
   assert.deepEqual(check.emptyRows,[]);assert(check.emptyHidden);assert.deepEqual(check.smallRows,[]);assert(check.smallHidden);assert.deepEqual(check.noDiskRows,[]);assert(check.noDiskHidden);
   assert.deepEqual(check.boundaryRows.map(row=>row.path),['/above'],'only totals strictly greater than 10 GB qualify');
   assert.deepEqual(check.layout,{summaryAbsent:true,lastResource:true,ioAboveNetwork:true,matchedStyle:true,filesystemsVisible:true});
   const narrow=await page.evaluate(stats=>{
    const property='--sidebar-width',previous=document.documentElement.style.getPropertyValue(property);
    document.documentElement.style.setProperty(property,'240px');
    const path='/data/projects/very-long-mount-name/<archive>& backups';
    renderMonitor({...stats,disks:[...stats.disks,{path,used:1024**3,total:20*1024**3,available:19*1024**3}]});
    const box=document.querySelector('#disk-filesystems'),last=box.lastElementChild,label=last.querySelector('span');
    const contained=[...box.children].every(row=>{
     const bar=row.querySelector('.meter').getBoundingClientRect(),percent=row.querySelector('b').getBoundingClientRect(),capacity=row.querySelector('em').getBoundingClientRect();
     return percent.right<=capacity.left&&bar.right<=box.getBoundingClientRect().right+1;
    });
    const result={contained,path:label.textContent,title:label.title,ellipsized:label.scrollWidth>label.clientWidth,markup:!!last.querySelector('archive')};
    if(previous)document.documentElement.style.setProperty(property,previous);else document.documentElement.style.removeProperty(property);
    renderMonitor(stats);return result;
   },diskFixture);
   assert(narrow.contained,'narrow-sidebar capacity and percentage must not overlap or overflow');assert.equal(narrow.path,'/data/projects/very-long-mount-name/<archive>& backups');assert.equal(narrow.title,narrow.path);assert(narrow.ellipsized);assert(!narrow.markup);
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   await (await page.$('.sidebar')).screenshot({path:process.env.DENGSHELL_MONITOR_SCREENSHOT?process.env.DENGSHELL_MONITOR_SCREENSHOT+'-'+theme+'.png':join(stage,'disk-capacity-'+theme+'.png')});
   if(process.env.DENGSHELL_MONITOR_SCREENSHOT)await (await page.$('#disk-filesystems')).screenshot({path:process.env.DENGSHELL_MONITOR_SCREENSHOT+'-mounts-'+theme+'.png'});
   diskChecks.push(check);
  }
 }
 assert.deepEqual(errors,[]);await writeFile(join(stage,'process-ui-test.json'),JSON.stringify({passed:true,...result,diskChecks,errors},null,2));console.log('PASS: permanent CPU/memory top five, stale cache label, unavailable exact RSS, 60-second window'+(diskChecks.length?', large mounts without summary, strict 10 GB threshold, single mount, both themes, narrow layout, cleared tab state and independent I/O rates.':'.'));
} finally {try{await browser?.close();}catch{}try{child.kill();}catch{}try{fs.rmSync(profileDir,{recursive:true,force:true,maxRetries:10,retryDelay:150});}catch{}}
