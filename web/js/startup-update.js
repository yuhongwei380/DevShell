// The fixed-host metadata probe runs alongside native startup in Go.
(() => {
 'use strict';let resolveStartup;window.DevShellStartupReady=new Promise(resolve=>{resolveStartup=resolve});
 function showUpdate(result){return new Promise(resolve=>{
  const dialog=document.createElement('dialog');dialog.id='startup-update-dialog';dialog.className='manager-dialog startup-update-dialog';dialog.setAttribute('aria-labelledby','startup-update-title');
  dialog.innerHTML='<div class="dialog-heading"><div><h2 id="startup-update-title">发现 DevShell 更新</h2><p id="startup-update-versions"></p></div><button type="button" class="icon-button" aria-label="稍后再说"><svg><use href="#i-close"/></svg></button></div><div class="startup-update-content"><p class="startup-update-source">更新来源：GitHub Releases · SHA-256 校验</p><h3>更新内容</h3><p id="startup-update-notes"></p><p class="startup-update-install-note"></p><div id="startup-update-progress" hidden><progress max="100" value="0"></progress><p role="status"></p></div><p id="startup-update-error" role="alert"></p></div><div class="startup-update-actions"><button type="button" class="secondary-button" id="startup-update-later">稍后再说</button><button type="button" class="primary-button" id="startup-update-install">下载并安装更新</button></div>';
  dialog.querySelector('.startup-update-source').textContent='更新来源：GitHub Releases（yuhongwei380/DevShell）· HTTPS 与 SHA-256 校验';if(result.mirror)dialog.querySelector('.startup-update-source').textContent=`更新来源：加速域名 ${result.mirror} · HTTPS 与 SHA-256 校验`;
  const versionLabel=(version,build)=>build?`${version}（构建 ${build}）`:version;dialog.querySelector('#startup-update-versions').textContent=`${versionLabel(result.currentVersion,result.currentBuild)} → ${versionLabel(result.latestVersion,result.latestBuild)}`;dialog.querySelector('#startup-update-notes').textContent=result.notes||'发布者尚未提供更新说明。';dialog.querySelector('.startup-update-install-note').textContent=result.platform.startsWith('linux')?`确认后下载并校验${result.package?.format==='pacman'?' Arch 安装包':' DEB 安装包'}，关闭当前 SSH 会话，再打开系统安装授权。安装后继续使用当前数据目录。`:'确认后下载并校验程序，关闭当前 SSH 会话，保留旧程序副本并自动重新启动。';
  window.DengReleaseNoteTags?.highlight(dialog.querySelector('#startup-update-notes'));
  const manual=result.installerReady===false;if(manual)dialog.querySelector('.startup-update-install-note').textContent=result.reason||'请从 GitHub Releases 下载对应安装包，保留原数据目录。';
  const install=dialog.querySelector('#startup-update-install'),later=dialog.querySelector('#startup-update-later'),close=dialog.querySelector('.icon-button'),progress=dialog.querySelector('#startup-update-progress');let busy=false,jobID='',stopped=false;
  const dismiss=()=>{stopped=true;dialog.close();if(busy&&jobID)remove(`/api/updates/download/${jobID}`).catch(()=>{})};later.onclick=close.onclick=dismiss;dialog.oncancel=event=>{event.preventDefault();dismiss()};
  if(manual)install.textContent='前往 Releases 下载';
  install.disabled=!manual&&(!desktopPage||!native()?.InstallUpdate);if(install.disabled)install.title='请在 DevShell 桌面应用中安装更新';
  install.onclick=async()=>{if(manual){try{if(native()?.OpenAbout)await native().OpenAbout();else window.open('https://github.com/yuhongwei380/DevShell/releases','_blank','noopener');}catch(error){dialog.querySelector('#startup-update-error').textContent=error.message||String(error);}return;}busy=true;install.disabled=true;progress.hidden=false;progress.querySelector('progress').hidden=false;progress.querySelector('progress').value=0;later.textContent='取消下载';dialog.querySelector('#startup-update-error').textContent='';let offProgress;try{
   let job=await post('/api/updates/download',{sha256:result.package.sha256});jobID=job.id;if(stopped){await remove(`/api/updates/download/${jobID}`).catch(()=>{});return}
   while(!stopped&&job.status==='downloading'){progress.querySelector('progress').value=job.total?job.received/job.total*100:0;progress.querySelector('p').textContent=`正在下载 ${Math.round(job.received/1048576)} / ${Math.ceil(job.total/1048576)} MB`;await new Promise(r=>setTimeout(r,250));job=await api(`/api/updates/download/${jobID}`)}
   if(stopped)return;if(job.status!=='ready')throw Error(job.error||'下载未完成');progress.querySelector('progress').removeAttribute('value');progress.querySelector('p').textContent='下载完成，正在准备安装，请稍候…';later.disabled=close.disabled=true;dialog.oncancel=event=>event.preventDefault();
   await window.DengPortablePreferences?.flush?.();await appearanceSave;offProgress=window.runtime?.EventsOn?.('dengshell:update-progress',message=>{if(typeof message==='string')progress.querySelector('p').textContent=message});await native().InstallUpdate(jobID);
  }catch(error){busy=false;install.disabled=false;later.disabled=close.disabled=false;later.textContent='稍后再说';dialog.oncancel=event=>{event.preventDefault();dismiss()};dialog.querySelector('#startup-update-error').textContent=error.message||String(error);progress.querySelector('progress').hidden=true;progress.querySelector('p').textContent='安装未开始，程序仍在运行，可以稍后重试。'}finally{if(typeof offProgress==='function')offProgress()}};
  dialog.addEventListener('close',()=>{stopped=true;dialog.remove();resolve()},{once:true});document.body.append(dialog);dialog.showModal();later.focus();
 })}
 let checking=false;
async function checkNow(){
 if(checking)return;checking=true;toast('正在检查更新…');
 try{
  const result=await post('/api/updates/check',{});
  if(result?.status==='available'){await showUpdate(result);return;}
  const label=(version,build)=>build?`${version}（构建 ${build}）`:version;
  if(result?.reason==='current'||result?.reason==='build-rollback'||result?.reason==='version-rollback')toast(`已是最新版本 ${label(result.currentVersion,result.currentBuild)}`);
  else{const reasonTexts={unavailable:'网络不可用或无法访问更新源',timeout:'检查超时，请稍后再试','release-not-found':'更新源中未找到匹配的 Release 资产','invalid-receipt':'本地更新记录损坏',invalid:'更新描述校验未通过','invalid-signature':'更新签名校验未通过','platform-unavailable':'当前平台未提供内置更新，请从 GitHub Releases 下载对应安装包升级，原数据目录会保留','linux-discontinued':'Linux 版已停止发布更新，当前版本可继续使用；如需升级请改用 Windows / macOS 版，旧版安装包仍可在 Releases 历史版本中获取'};const mirrorHint=['unavailable','timeout','release-not-found'].includes(result?.reason)?'；可在「更新加速域名」中填写 GitHub 加速地址后重试':'';toast(`检查更新失败：${reasonTexts[result?.reason]||'未知错误'}${mirrorHint}`);}
 }catch(error){toast(`检查更新失败：${error.message||String(error)}`)}
 finally{checking=false}
}
window.DengUpdates={checkNow,showUpdate,mirrorSettings};
 // GitHub accelerator settings: ghproxy-style prefix hosts tried before the
 // canonical GitHub URLs. Manifest trust (product/platform/build/SHA-256) and
 // the downloaded-file SHA-256 check are unchanged; a failing accelerator
 // still falls back to direct GitHub.
const MIRROR_PRESETS=[['直连 GitHub（默认）',''],['gh-proxy.org','https://gh-proxy.org'],['gh-proxy.com','https://gh-proxy.com'],['ghfast.top','https://ghfast.top'],['mirror.mikus.ink','https://mirror.mikus.ink']];
function mirrorSettings(){return new Promise(resolve=>{
 const dialog=document.createElement('dialog');dialog.className='manager-dialog startup-update-dialog';dialog.id='update-mirror-dialog';
 dialog.innerHTML='<div class="dialog-heading"><div><h2>GitHub 更新加速域名</h2><p id="update-mirror-note">网络受限时可改用镜像下载更新，清单与安装包仍强制校验</p></div><button type="button" class="icon-button" aria-label="关闭加速设置"><svg><use href="#i-close"/></svg></button></div><div class="update-mirror-body"><label for="update-mirror-preset">常用加速域名</label><select id="update-mirror-preset">'+MIRROR_PRESETS.map(([label,value])=>`<option value="${value}">${label}</option>`).join('')+'<option value="__custom">自定义…</option></select><label for="update-mirror-input">自定义地址（https，仅主机名）</label><input id="update-mirror-input" placeholder="例如 https://gh-proxy.org" spellcheck="false" autocomplete="off"><p class="update-mirror-help">下载仍会校验产品、平台、构建号与 SHA-256；加速域名失败时自动回落直连。留空即直连。</p><p id="update-mirror-error" role="alert"></p></div><div class="dialog-buttons"><button type="button" class="secondary-button" id="update-mirror-clear">清除（直连）</button><button type="button" class="primary-button" id="update-mirror-save">保存</button></div>';
 const preset=dialog.querySelector('#update-mirror-preset'),input=dialog.querySelector('#update-mirror-input'),error=dialog.querySelector('#update-mirror-error');
 const fill=value=>{const known=MIRROR_PRESETS.find(([,v])=>v===value);preset.value=known?value:'__custom';if(known)input.value='';else input.value=value||'';};
 const current=async()=>{try{const config=await api('/api/config');fill(config?.appearance?.updateMirror||'')}catch{fill('')}};
 const commit=async value=>{error.textContent='';try{await post('/api/appearance/patch',{updateMirror:String(value||'').trim()});toast(String(value||'').trim()?'已保存，检查与下载更新将优先使用该加速域名':'已清除加速域名，恢复直连 GitHub');dialog.close();}catch(reason){error.textContent=reason?.message||String(reason)}};
 preset.onchange=()=>{if(preset.value!=='__custom')input.value=''};
 dialog.querySelector('#update-mirror-save').onclick=()=>commit(preset.value==='__custom'?input.value:preset.value);
 dialog.querySelector('#update-mirror-clear').onclick=()=>{fill('');commit('')};
 const close=()=>dialog.close();dialog.querySelector('.icon-button').onclick=close;dialog.oncancel=event=>{event.preventDefault();close()};
 dialog.addEventListener('close',()=>{dialog.remove();resolve()},{once:true});
 document.body.append(dialog);dialog.showModal();current();
});}
document.addEventListener('DOMContentLoaded',async()=>{let timer;try{if(window.DEVSHELL?.detachedNonce)return;const check=Promise.race([api('/api/updates/check').catch(()=>null),new Promise(resolve=>{timer=setTimeout(()=>resolve(null),3000)})]);const result=await check;clearTimeout(timer);if(result?.status==='available')await showUpdate(result);}catch(e){console.warn('启动更新检查未完成',e)}finally{clearTimeout(timer);resolveStartup();window.dispatchEvent(new Event('dengshell:startup-ready'))}},{once:true});
})();
