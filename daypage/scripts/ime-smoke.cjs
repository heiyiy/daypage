// Native foreground switching against isolated data; restores minimized user windows.
const {chromium}=require(process.env.DAYPAGE_PLAYWRIGHT_MODULE||'playwright');
const {execFileSync}=require('node:child_process');
const fs=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
const root=path.resolve(__dirname,'..');
const directory=path.join(root,'verification','test-data',randomUUID());
fs.mkdirSync(directory,{recursive:true});
const executable=process.env.DAYPAGE_EXE||path.join(root,'src-tauri/target/release/daypage.exe');
const env={...process.env,DAYPAGE_EXE:executable,DAYPAGE_DATA_DIR:directory,WEBVIEW2_USER_DATA_FOLDER:path.join(directory,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=19386'};
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let pid,browser,widget;
const invoke=(command,args={})=>widget.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
(async()=>{
 try{
  pid=Number(execFileSync('powershell.exe',['-NoProfile','-Command','(Start-Process -FilePath $env:DAYPAGE_EXE -WindowStyle Hidden -PassThru).Id'],{env,windowsHide:true,encoding:'utf8'}).trim());
  for(let i=0;i<100;i++){try{if((await fetch('http://127.0.0.1:19386/json/version')).ok)break;}catch{}await delay(200);}
  browser=await chromium.connectOverCDP('http://127.0.0.1:19386');
  widget=browser.contexts()[0].pages()[0];
  const errors=[];widget.on('pageerror',e=>errors.push(e.message));
  await widget.locator('#task-input:enabled').waitFor();
  const day=await widget.evaluate(()=>{const d=new Date();return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;});
  const task=await invoke('create_task',{date:day,title:'IME regression'});
  const row=widget.locator('#task-list li').filter({hasText:'IME regression'});
  await row.locator('.entry-menu').click();
  const field=row.locator('.reflection-input');
  await field.focus();
  await widget.evaluate(()=>{window.imeField=document.querySelector('.reflection-input');window.imeEvents=[];for(const name of ['compositionstart','compositionend','blur'])window.imeField.addEventListener(name,()=>window.imeEvents.push(name));});
  const cdp=await browser.contexts()[0].newCDPSession(widget);
  await cdp.send('Input.imeSetComposition',{text:'fansi',selectionStart:5,selectionEnd:5});
  await delay(1000); // Composition must outlive the normal 500 ms autosave interval.
  assert.equal((await invoke('get_day',{date:day})).find(r=>r.id===task.id).reflection,'');
  await invoke('create_task',{date:day,title:'External refresh during composition'});
  await delay(300);
  assert(await widget.evaluate(()=>window.imeField===document.querySelector('.reflection-input')&&!window.imeField.disabled&&document.activeElement===window.imeField));
  assert.equal(await widget.evaluate(()=>window.imeEvents.filter(e=>e==='compositionend').length),0);
  await cdp.send('Input.insertText',{text:'反思第一句'});
  for(let i=0;i<50;i++){if((await invoke('get_day',{date:day})).find(r=>r.id===task.id).reflection==='反思第一句')break;await delay(100);}
  assert.equal((await invoke('get_day',{date:day})).find(r=>r.id===task.id).reflection,'反思第一句');
  assert(await widget.evaluate(()=>window.imeField===document.querySelector('.reflection-input')&&!window.imeField.disabled&&document.activeElement===window.imeField));
  // Hold the reply after the DB write, then continue composing while the first save is in flight.
  await widget.evaluate(()=>{const original=window.__TAURI_INTERNALS__.invoke;window.__TAURI_INTERNALS__.invoke=async function(command,args,...rest){const value=await original.call(this,command,args,...rest);if(command==='edit_entry'&&args.action==='reflection'){await new Promise(r=>setTimeout(r,1200));}return value;};});
  await field.fill('已保存前缀');await delay(650);
  await cdp.send('Input.imeSetComposition',{text:'jixu',selectionStart:4,selectionEnd:4});
  await delay(1600);
  assert(await widget.evaluate(()=>window.imeField===document.querySelector('.reflection-input')&&!window.imeField.disabled&&document.activeElement===window.imeField));
  assert.equal(await widget.evaluate(()=>window.imeEvents.filter(e=>e==='compositionend').length),1);
  await cdp.send('Input.insertText',{text:'继续中文'});
  const expected=await field.inputValue();assert(expected.includes('继续中文'));
  for(let i=0;i<70;i++){if((await invoke('get_day',{date:day})).find(r=>r.id===task.id).reflection===expected)break;await delay(100);}
  assert.equal((await invoke('get_day',{date:day})).find(r=>r.id===task.id).reflection,expected);
  assert(await widget.evaluate(()=>window.imeField===document.querySelector('.reflection-input')&&!window.imeField.disabled));
  // Handler closures must use the new row version after background autosaves.
  await row.locator('.task-editor input').fill('Renamed after reflection');
  await row.getByRole('button',{name:'保存',exact:true}).click();
  for(let i=0;i<40;i++){if((await invoke('get_day',{date:day})).find(r=>r.id===task.id).title==='Renamed after reflection')break;await delay(100);}
  assert.equal((await invoke('get_day',{date:day})).find(r=>r.id===task.id).title,'Renamed after reflection');
  assert.deepEqual(errors,[]);
  await invoke('quit_app');
  console.log('PASS: browser IME composition survives autosave debounce, external task refresh and delayed save replies; editor node/focus stay stable, latest draft persists, subsequent title edit uses latest version.');
 }finally{
  if(browser)await browser.close().catch(()=>{});
  if(pid)execFileSync('powershell.exe',['-NoProfile','-Command',`$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($p -and $p.Path -eq $env:DAYPAGE_EXE){$p.Kill()}; exit 0`],{env,windowsHide:true});
 }
})().catch(error=>{console.error(error);process.exitCode=1;});
