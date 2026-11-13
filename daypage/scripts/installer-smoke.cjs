const {chromium}=require(process.env.DAYPAGE_PLAYWRIGHT_MODULE||'playwright');
const {execFileSync}=require('node:child_process');
const {DatabaseSync}=require('node:sqlite');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const root=path.resolve(__dirname,'..');
const installer=path.join(root,'src-tauri/target/release/bundle/nsis/日页_0.2.0_x64-setup.exe');
const reference=path.join(root,'src-tauri/target/release/daypage.exe');
const id=crypto.randomUUID(),base=path.join(root,'verification',`install-${id}`),target=path.join(base,'app'),data=path.join(base,'data');
const executable=path.join(target,'daypage.exe'),dbPath=path.join(data,'daypage.sqlite3');
const port=19384;
const ps=(script,args=[])=>execFileSync('powershell.exe',['-NoProfile','-Command','[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding $false;'+script,...args],{windowsHide:true,encoding:'utf8'}).trim();
const env={...process.env,DAYPAGE_EXE:executable,DAYPAGE_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(base,'webview'),DAYPAGE_AUTOSTART_VALUE:`DaypageAcceptance-${id}`,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`};
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
function assertPayload(){
  const expected=fs.readFileSync(reference),actual=fs.readFileSync(executable);
  // Tauri changes exactly this three-byte marker for the NSIS payload, then restores the source.
  const marker=Buffer.from('__TAURI_BUNDLE_TYPE_VAR_UNK'),offset=expected.indexOf(marker);
  assert(offset>=0);assert.equal(actual.subarray(offset,offset+marker.length).toString(),'__TAURI_BUNDLE_TYPE_VAR_NSS');
  marker.copy(actual,offset);assert.deepEqual(actual,expected,'Installed code must match the built program aside from its bundle-type marker');
}
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const uninstallKey='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\日页';
const manufacturerKey='HKCU:\\Software\\personal\\日页';
const startupKey='HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run';
let browser,pid,installed=false,originalStartup,oldManufacturer,shortcuts=[];
async function launch(){
  pid=Number(execFileSync('powershell.exe',['-NoProfile','-Command','(Start-Process -FilePath $env:DAYPAGE_EXE -WindowStyle Hidden -PassThru).Id'],{env,windowsHide:true,encoding:'utf8'}).trim());
  for(let n=0;n<80;n++){try{if((await fetch(`http://127.0.0.1:${port}/json/version`)).ok)break;}catch{}await pause(150);}
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const widget=browser.contexts()[0].pages()[0];await widget.locator('#task-input:enabled').waitFor();
  const invoke=(cmd,args={})=>widget.evaluate(({cmd,args})=>window.__TAURI_INTERNALS__.invoke(cmd,args),{cmd,args});
  const tasks=await invoke('get_day',{date:'2026-11-12'}),note=await invoke('get_note',{date:'2026-11-12'});
  assert.equal(tasks[0].title,'升级前待办');assert.equal(tasks[0].reflection,'');assert.equal(note.body,'升级前日记');
  await invoke('quit_app').catch(()=>{});await browser.close();browser=undefined;
  execFileSync('powershell.exe',['-NoProfile','-Command',`$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue;if($p -and -not $p.WaitForExit(10000)){throw 'Test app did not exit'};exit 0`],{windowsHide:true});pid=undefined;
}
function operation(uninstall=false){
 const args=['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(root,'scripts/installer-operation.ps1'),'-Installer',installer,'-Target',target];if(uninstall)args.push('-Uninstall');else if(fs.existsSync(executable))args.push('-Update');
 execFileSync('powershell.exe',args,{windowsHide:true,encoding:'utf8',timeout:150000});
}
(async()=>{
 assert(fs.existsSync(installer));
 // Refuse to replace an existing installed copy; portable user data is separately fingerprinted.
 assert.equal(ps("if(Test-Path -LiteralPath 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\日页'){'exists'}else{'none'}"),'none','An existing user installation must not be replaced by the test');
 originalStartup=ps("$key=Get-Item -LiteralPath 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run';$v=$key.GetValue('Daypage');ConvertTo-Json -InputObject $v -Compress");
 oldManufacturer=ps("if(Test-Path -LiteralPath 'HKCU:\\Software\\personal\\日页'){'exists'}else{'none'}");
 const userDb=path.join(process.env.APPDATA,'com.personal.daypage/daypage.sqlite3');const userHash=fs.existsSync(userDb)?hash(userDb):null;
 const shortcutPaths=JSON.parse(ps("@(([Environment]::GetFolderPath('Desktop')+'\\日页.lnk'),([Environment]::GetFolderPath('Programs')+'\\日页.lnk'))|ConvertTo-Json -Compress"));
 shortcuts=shortcutPaths.map(file=>({file,bytes:fs.existsSync(file)?fs.readFileSync(file):null}));
 fs.mkdirSync(data,{recursive:true});
 if(oldManufacturer==='exists')execFileSync('reg.exe',['export','HKCU\\Software\\personal\\日页',path.join(base,'manufacturer.reg'),'/y'],{windowsHide:true,stdio:'ignore'});
 const db=new DatabaseSync(dbPath);db.exec(fs.readFileSync(path.join(root,'src-tauri/migrations/001_initial.sql'),'utf8'));db.exec(fs.readFileSync(path.join(root,'src-tauri/migrations/002_journal.sql'),'utf8'));
 const now=new Date().toISOString();
 db.prepare('INSERT INTO tasks(id,title,created_at,updated_at) VALUES(?,?,?,?)').run('upgrade-task','升级前待办',now,now);
 db.prepare('INSERT INTO task_days(id,task_id,date,title_snapshot,sort_order,created_at,updated_at) VALUES(?,?,?,?,?,?,?)').run('upgrade-day','upgrade-task','2026-11-12','升级前待办',0,now,now);
 db.prepare('INSERT INTO daily_notes(date,body,updated_at) VALUES(?,?,?)').run('2026-11-12','升级前日记',now);db.close();
 try{
   installed=true;operation();
   assertPayload();console.log('Install passed.');
   await launch();assert(fs.existsSync(path.join(data,'daypage.pre-v3.sqlite3')));
   const upgraded=new DatabaseSync(dbPath);assert.equal(upgraded.prepare('PRAGMA user_version').get().user_version,3);upgraded.close();
   execFileSync('reg.exe',['add',startupKey,'/v','Daypage','/t','REG_SZ','/d',`"${executable}"`,'/f'],{windowsHide:true,stdio:'ignore'});
   const before=hash(dbPath);operation();assert.equal(hash(dbPath),before);assertPayload();
   assert.equal(JSON.parse(ps("Get-ItemPropertyValue -LiteralPath 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name Daypage | ConvertTo-Json -Compress")),`"${executable}"`);
   await launch();console.log('Overwrite install and v2 migration passed.');
   operation(true);installed=false;
   for(let n=0;n<60&&fs.existsSync(executable);n++)await pause(100);
   assert(!fs.existsSync(executable));assert(fs.existsSync(dbPath));
   const saved=new DatabaseSync(dbPath);assert.equal(saved.prepare('SELECT body FROM daily_notes').get().body,'升级前日记');saved.close();
   if(userHash!==null)assert.equal(hash(userDb),userHash,'User database must remain untouched');else assert(!fs.existsSync(userDb));
   fs.writeFileSync(path.join(root,'verification/installer.json'),JSON.stringify({version:'0.2.0',install:true,overwrite:true,v2Migration:true,preMigrationSnapshot:true,uninstallPreservesData:true,userDataUnchanged:true},null,2));
   console.log('PASS: current-user install, v2 migration, overwrite, uninstall preserves data, original user database unchanged.');
 }finally{
   if(browser)try{await browser.close();}catch{}
   if(pid)try{execFileSync('powershell.exe',['-NoProfile','-Command',`$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue;if($p -and $p.Path -eq $env:DAYPAGE_EXE){$p.Kill();$p.WaitForExit()};exit 0`],{env,windowsHide:true});}catch{}
   if(installed)operation(true);
   for(const item of shortcuts){if(item.bytes)fs.writeFileSync(item.file,item.bytes);else if(fs.existsSync(item.file))fs.unlinkSync(item.file);}
   if(originalStartup&&originalStartup!=='null')execFileSync('reg.exe',['add',startupKey,'/v','Daypage','/t','REG_SZ','/d',JSON.parse(originalStartup),'/f'],{windowsHide:true,stdio:'ignore'});
   else try{execFileSync('reg.exe',['delete',startupKey,'/v','Daypage','/f'],{windowsHide:true,stdio:'ignore'});}catch{}
   if(oldManufacturer==='exists')execFileSync('reg.exe',['import',path.join(base,'manufacturer.reg')],{windowsHide:true,stdio:'ignore'});
   if(oldManufacturer==='none')ps("Remove-Item -LiteralPath 'HKCU:\\Software\\personal\\日页' -Recurse -Force -ErrorAction SilentlyContinue;exit 0");
 }
})().catch(error=>{console.error(error);process.exitCode=1;});
