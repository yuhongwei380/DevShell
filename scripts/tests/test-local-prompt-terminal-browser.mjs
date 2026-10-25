import fs from 'node:fs';
import assert from 'node:assert/strict';
const [stage,chrome,modulePath]=process.argv.slice(2);
const {default:puppeteer}=await import(modulePath),fixture=JSON.parse(fs.readFileSync(stage+'/browser-fixture.json'));
const browser=await puppeteer.launch({executablePath:chrome,headless:true,args:['--no-sandbox','--disable-dev-shm-usage']});
try {
 const page=await browser.newPage();await page.setViewport({width:1280,height:800});await page.goto(fixture.url,{waitUntil:'networkidle0'});
 const lines=await page.evaluate(async()=>{
  document.querySelectorAll('dialog[open]').forEach(d=>d.close());
  // A detached xterm is enough: the bug is how termSink feeds newlines to the
  // terminal parser, which happens before any renderer is involved.
  const term=new Terminal({cols:80,rows:24,allowProposedApi:false});
  const sink=window.DengLocalPrompt.termSink(term);
  sink('[本机] $ ping baidu.com\n','cmd');
  sink('本机 ICMP → baidu.com (110.242.74.102)\n1  110.242.74.102  51.00 ms\n2  110.242.74.102  50.00 ms\n收到 2 / 2 个响应\n\n');
  await new Promise(resolve=>term.write('',resolve)); // drain the async write queue
  const out=[];
  for(let i=0;i<term.buffer.active.length;i++){const line=term.buffer.active.getLine(i);if(line){const text=line.translateToString(true);if(text)out.push(text);}}
  term.dispose();
  return out;
 });
 assert.deepEqual(lines,[
  '[本机] $ ping baidu.com',
  '本机 ICMP → baidu.com (110.242.74.102)',
  '1  110.242.74.102  51.00 ms',
  '2  110.242.74.102  50.00 ms',
  '收到 2 / 2 个响应',
 ],'multi-line local prompt output must stay left-aligned in the terminal');
 // End-to-end: a non-ping local command routes through run() to the backend and
 // renders left-aligned in the terminal too. nslookup must end with a
 // completion line so users know the lookup finished.
 const probe=await page.evaluate(async()=>{
  const term=new Terminal({cols:80,rows:24,allowProposedApi:false});
  await window.DengLocalPrompt.run('nslookup 127.0.0.1',window.DengLocalPrompt.termSink(term));
  await new Promise(resolve=>term.write('',resolve));
  const out=[];for(let i=0;i<term.buffer.active.length;i++){const l=term.buffer.active.getLine(i);if(l){const t=l.translateToString(true);if(t)out.push(t);}}
  term.dispose();return out;
 });
 assert(probe.some(l=>l.includes('本机 DNS → 127.0.0.1'))&&probe.some(l=>l.includes('本身即 IP')),'nslookup must route to the local prompt and render its output: '+JSON.stringify(probe));
 assert(probe.some(l=>l.includes('解析完成')),'nslookup must print a completion hint: '+JSON.stringify(probe));
 // dig routes through the local prompt and prints a dig-style answer.
 const dig=await page.evaluate(async()=>{
  const term=new Terminal({cols:80,rows:24,allowProposedApi:false});
  await window.DengLocalPrompt.run('dig 127.0.0.1 A',window.DengLocalPrompt.termSink(term));
  await new Promise(resolve=>term.write('',resolve));
  const out=[];for(let i=0;i<term.buffer.active.length;i++){const l=term.buffer.active.getLine(i);if(l){const t=l.translateToString(true);if(t)out.push(t);}}
  term.dispose();return out;
 });
 assert(dig.some(l=>l.includes('QUESTION SECTION'))&&dig.some(l=>l.includes('127.0.0.1'))&&dig.some(l=>l.includes('IN')),'dig must route to the local prompt and render its answer: '+JSON.stringify(dig));
 // curl accepts a scheme-less target and defaults to http, like the real curl:
 // hit the fixture server's own address over plain http. The probe sends no
 // session token, so any HTTP status line proves the http:// default resolved
 // and connected (the functional 200 path is covered by the Go test).
 const curlOut=await page.evaluate(async()=>{
  const bare=window.DEVSHELL.base.replace(/^http:\/\//,'')+'/boot.js';
  const term=new Terminal({cols:80,rows:24,allowProposedApi:false});
  await window.DengLocalPrompt.run('curl '+bare,window.DengLocalPrompt.termSink(term));
  await new Promise(resolve=>term.write('',resolve));
  const out=[];for(let i=0;i<term.buffer.active.length;i++){const l=term.buffer.active.getLine(i);if(l){const t=l.translateToString(true);if(t)out.push(t);}}
  term.dispose();return out;
 });
 assert(curlOut.some(l=>/HTTP\/1\.[01] \d{3}/.test(l)),'scheme-less curl must default to http: '+JSON.stringify(curlOut));
 // The empty-box placeholder must never scroll (single line, clipped).
 const placeholder=await page.evaluate(()=>{
  const ta=document.querySelector('#command-input');
  const cs=getComputedStyle(ta);
  return {shown:ta.matches(':placeholder-shown'),whiteSpace:cs.whiteSpace,scrollable:ta.scrollHeight>ta.clientHeight+1};
 });
 assert(placeholder.shown&&placeholder.whiteSpace==='nowrap'&&!placeholder.scrollable,'placeholder must stay single-line without a scrollbar: '+JSON.stringify(placeholder));
 // Single-line typed input must sit exactly where the placeholder sat: same
 // font size and same vertical padding, so the first keystroke neither jumps
 // to the top nor reads as a smaller font. Only real multi-line editing
 // (Shift + Enter or wrapping) top-aligns with tighter padding.
 const align=await page.evaluate(()=>{
  const ta=document.querySelector('#command-input');const bar=ta.closest('.terminal-command-bar');
  const empty={font:getComputedStyle(ta).fontSize,padTop:getComputedStyle(ta).paddingTop};
  ta.value='d';ta.dispatchEvent(new Event('input',{bubbles:true}));
  const typed={font:getComputedStyle(ta).fontSize,padTop:getComputedStyle(ta).paddingTop,multiline:bar.classList.contains('multiline')};
  ta.value='d\ne';ta.dispatchEvent(new Event('input',{bubbles:true}));
  const multi={multiline:bar.classList.contains('multiline'),padTop:getComputedStyle(ta).paddingTop};
  ta.value='';ta.dispatchEvent(new Event('input',{bubbles:true}));
  return {empty,typed,multi};
 });
 assert(align.typed.font==='13px'&&align.typed.padTop===align.empty.padTop&&!align.typed.multiline,'single-line typed input must keep the 13px base size and the placeholder vertical center: '+JSON.stringify(align));
 assert(align.multi.multiline&&align.multi.padTop==='5px','multi-line editing must top-align: '+JSON.stringify(align));
 fs.writeFileSync(stage+'/local-prompt-terminal.json',JSON.stringify({passed:true,noStaircase:true,nslookupRouted:true,nslookupCompletion:true,digRouted:true,curlSchemeLess:true,placeholderNoScroll:true,inputStaysCentered:true}));console.log('PASS local prompt CRLF, nslookup completion hint, dig, scheme-less curl, placeholder no-scroll, input stays centered');
}finally{await browser.close()}
