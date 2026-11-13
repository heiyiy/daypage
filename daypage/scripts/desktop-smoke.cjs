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
const executable=path.join(root,'src-tauri/target/release/daypage.exe');
const env={...process.env,DAYPAGE_EXE:executable,DAYPAGE_DATA_DIR:directory,WEBVIEW2_USER_DATA_FOLDER:path.join(directory,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=19385'};
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let pid,browser,widget;
const invoke=(command,args={})=>widget.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
const probe=action=>JSON.parse(execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(__dirname,'desktop-probe.ps1'),'-AppPid',String(pid),'-Action',action],{windowsHide:true,encoding:'utf8'}).trim());
async function visible(expected){for(let i=0;i<10;i++){const status=probe('status');if(status.visible===expected)return status;await delay(150);}throw new Error(`Expected visible=${expected}: ${JSON.stringify(probe('status'))}`);}
(async()=>{
 try{
  pid=Number(execFileSync('powershell.exe',['-NoProfile','-Command','(Start-Process -FilePath $env:DAYPAGE_EXE -WindowStyle Hidden -PassThru).Id'],{env,windowsHide:true,encoding:'utf8'}).trim());
  for(let i=0;i<100;i++){try{if((await fetch('http://127.0.0.1:19385/json/version')).ok)break;}catch{}await delay(200);}
  browser=await chromium.connectOverCDP('http://127.0.0.1:19385');
  widget=browser.contexts()[0].pages()[0];
  await widget.locator('#task-input:enabled').waitFor();
  assert.equal(await widget.locator('#pin').count(),0);
  console.log("Initial desktop",probe('desktop'));assert.equal((await visible(true)).topmost,false);
  await widget.locator('#task-input').fill('unsaved desktop draft');
  console.log('Widget focus',probe('widget'));assert.equal((await visible(true)).widgetFocused,true);
  const next=browser.contexts()[0].waitForEvent('page');await invoke('open_journal');const journal=await next;
  await journal.locator('.calendar-day').first().waitFor();
  await visible(false);
  for(let i=0;i<3;i++){
   console.log('Desktop cycle',i,probe('desktop'));await visible(true);
   console.log('Journal cycle',i,probe('journal'));await visible(false);
  }
  probe('journal');await visible(false);
  console.log('Win+D',probe('winkeyd'));await visible(true);
  assert.equal(await widget.locator('#task-input').inputValue(),'unsaved desktop draft');
  await invoke('hide_widget');await visible(false);
  probe('journal');probe('desktop');await visible(false);
  await invoke('show_widget');await visible(true);
  // Re-enabling via tray while another app has focus must not cover that app.
  probe('journal');await visible(false);await invoke('show_widget');await visible(false);
  probe('desktop');await visible(true);
  await widget.locator('#task-input').fill('');
  await invoke('quit_app');
  console.log('PASS: desktop-only visibility, application/journal switching, Win+D-style desktop return, no topmost, draft retention, manual hide and tray re-enable.');
 }finally{
  if(pid){try{probe('restore');}catch{}}
  if(browser)await browser.close().catch(()=>{});
  if(pid)execFileSync('powershell.exe',['-NoProfile','-Command',`$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($p -and $p.Path -eq $env:DAYPAGE_EXE){$p.Kill()}; exit 0`],{env,windowsHide:true});
 }
})().catch(error=>{console.error(error);process.exitCode=1;});
