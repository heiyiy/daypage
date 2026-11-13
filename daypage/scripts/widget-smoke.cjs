// Real Windows WebView2 acceptance. All database fixtures have a unique run ID.
const {chromium}=require(process.env.DAYPAGE_PLAYWRIGHT_MODULE||'playwright');
const {execFileSync}=require('node:child_process');
const {DatabaseSync}=require('node:sqlite');
const fs=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
const root=path.resolve(__dirname,'..');
const executable=process.env.DAYPAGE_EXE||path.join(root,'src-tauri/target/release/daypage.exe');
const runId=randomUUID();
const title=`测试待办 ${runId}`;
const noteText=`测试日记 ${runId}`;
const fixtureTitles=Array.from({length:5},(_,i)=>`月历验收 ${i+1} ${runId}`);
const port=19383;
const testDirectory=path.join(root,'verification','test-data',runId);
fs.mkdirSync(testDirectory,{recursive:true});
const layoutPath=path.join(testDirectory,'window-layout.json');
const originalLayout=fs.existsSync(layoutPath)?fs.readFileSync(layoutPath):null;
const geometry=page=>page.evaluate(async()=>{const w=window.__TAURI__.window.getCurrentWindow();const p=await w.outerPosition(),s=await w.innerSize();return {x:p.x,y:p.y,width:s.width,height:s.height};});
let expectedLayout;
const dbPath=path.join(testDirectory,'daypage.sqlite3');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let pid,browser,widget,journal,noteDate,initialTheme,initialOpacity,today;
const errors=[];
let passed=false;
const env={...process.env,DAYPAGE_DATA_DIR:testDirectory,WEBVIEW2_USER_DATA_FOLDER:path.join(testDirectory,"webview"),DAYPAGE_AUTOSTART_VALUE:`DaypageAcceptance-${runId}`,DAYPAGE_EXE:executable,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`};
const invoke=(page,cmd,args={})=>page.evaluate(({cmd,args})=>window.__TAURI_INTERNALS__.invoke(cmd,args),{cmd,args});
const opacity=()=>widget.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--panel-opacity').trim());
async function setOpacity(target){
  for(let i=0;i<3 && Number(await opacity())!==Number(target);i++)await widget.locator('#opacity').click();
  assert.equal(Number(await opacity()),Number(target));
}
async function restoreAppearance(){
  if(widget&&!widget.isClosed()&&initialTheme!==undefined){
    await widget.locator('#widget-theme').selectOption(initialTheme);
    await setOpacity(initialOpacity);
  }
}
async function start(){
  console.log('Starting test application...');
  pid=Number(execFileSync('powershell.exe',['-NoProfile','-Command','(Start-Process -FilePath $env:DAYPAGE_EXE -WindowStyle Hidden -PassThru).Id'],{env,windowsHide:true,encoding:'utf8'}).trim());
  for(let i=0;i<80;i++){
    try{if((await fetch(`http://127.0.0.1:${port}/json/version`)).ok)break;}catch{}
    await delay(250);
  }
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  widget=browser.contexts()[0].pages()[0];
  widget.on('pageerror',error=>errors.push(error.message));
  await widget.locator('body.widget[data-theme] #task-input:enabled').waitFor();
  today=await widget.evaluate(()=>{const d=new Date();return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;});
  console.log('Widget ready.');
}
async function openJournal(){
  const next=browser.contexts()[0].waitForEvent('page');
  await widget.locator('#open-journal').click();
  journal=await next;
  journal.on('pageerror',error=>errors.push(error.message));
  await journal.locator('.calendar-day').first().waitFor();
  assert.equal(await journal.locator('#day-dialog').evaluate(el=>el.open),false);
}
async function openDay(date){
  await journal.locator(`.calendar-day[data-date="${date}"]`).click();
  await waitDay(date);
}
async function waitDay(date){
  await journal.waitForFunction(date=>document.querySelector('#day-dialog').open&&!document.querySelector('#note').disabled&&document.querySelector('.calendar-day.selected')?.dataset.date===date,date);
}
async function previousMonth(){
  const before=await journal.locator('#month-label').textContent();
  await journal.locator('#prev-month').click();
  await journal.waitForFunction(before=>document.querySelector('#month-label').textContent!==before,before);
}
async function stop(){
  if(browser){
    const page=browser.contexts()[0]?.pages().find(page=>!page.isClosed());
    if(page){try{await invoke(page,'quit_app');}catch{}}
    try{await browser.close();}catch{}
    browser=undefined;
  }
  if(pid){
    execFileSync('powershell.exe',['-NoProfile','-Command',`$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($p -and -not $p.WaitForExit(5000)){if($p.Path -ne $env:DAYPAGE_EXE){throw '进程路径不匹配，拒绝停止'}; $p.Kill();$p.WaitForExit()}; exit 0`],{windowsHide:true,env});
    pid=undefined;
  }
}
function cleanup(){
  if(!fs.existsSync(dbPath))return;
  const db=new DatabaseSync(dbPath);
  try{
    db.exec('PRAGMA foreign_keys=ON; BEGIN IMMEDIATE');
    for(const text of [title,...fixtureTitles,fixtureTitles[0]+"（已修改）"]){
      for(const row of db.prepare('SELECT id,task_id FROM task_days WHERE title_snapshot=?').all(text)){
        db.prepare('DELETE FROM task_days WHERE id=?').run(row.id);
        db.prepare('DELETE FROM tasks WHERE id=? AND NOT EXISTS(SELECT 1 FROM task_days WHERE task_id=?)').run(row.task_id,row.task_id);
      }
    }
    if(noteDate)db.prepare('DELETE FROM daily_notes WHERE date=? AND body=?').run(noteDate,noteText);
    db.exec('COMMIT');
  }finally{db.close();}
}
(async()=>{
  const pe=fs.readFileSync(executable);
  assert.equal(pe.readUInt16LE(pe.readUInt32LE(0x3c)+24+68),2);
  try{
    await start();
    if(process.env.DAYPAGE_GEOMETRY_ONLY){
      console.log('Before',await geometry(widget));
      await stop();
      fs.writeFileSync(layoutPath,JSON.stringify({main:{x:40,y:40,width:445,height:558},journal:{x:60,y:60,width:1330,height:930}}));
      console.log('Seed',fs.readFileSync(layoutPath,'utf8'));
      await start();await delay(1000);
      console.log('Restored',await geometry(widget),'File',fs.readFileSync(layoutPath,'utf8'));
      console.log('Monitors',await widget.evaluate(async()=>({all:await window.__TAURI__.window.availableMonitors(),primary:await window.__TAURI__.window.primaryMonitor()})));
      return;
    }
    initialTheme=await widget.locator('#widget-theme').inputValue();initialOpacity=await opacity();
    assert.equal(browser.contexts()[0].pages().length,1);
    assert.equal(await widget.evaluate(()=>getComputedStyle(document.body).backgroundColor),'rgba(0, 0, 0, 0)');
    const pinned=await widget.locator('#pin').getAttribute('aria-pressed');
    await widget.locator('#pin').click();
    await widget.waitForFunction(value=>document.querySelector('#pin').getAttribute('aria-pressed')!==value,pinned);
    await widget.locator('#pin').click();
    await widget.waitForFunction(value=>document.querySelector('#pin').getAttribute('aria-pressed')===value,pinned);
    await widget.locator('#task-input').fill(title);await widget.locator('#task-input').press('Enter');
    const row=widget.locator('li').filter({hasText:title});await row.waitFor();
    for(const theme of ['original','light','dark']){
      await widget.locator('#widget-theme').selectOption(theme);
      for(let i=0;i<3;i++){
        const metrics=await row.locator('span').evaluate(el=>({color:getComputedStyle(el).color,opacity:getComputedStyle(el).opacity,weight:Number(getComputedStyle(el).fontWeight),size:parseFloat(getComputedStyle(el).fontSize)}));
        assert.equal(metrics.opacity,'1');assert(metrics.size>=14);assert(metrics.weight>=700);
        if(theme==='light')assert.equal(metrics.color,'rgb(0, 0, 0)');
        if(theme==='dark')assert.equal(metrics.color,'rgb(255, 255, 255)');
        await widget.locator('#opacity').click();
      }
    }
    await row.locator('input').click();
    await widget.waitForFunction(title=>[...document.querySelectorAll('li.done')].some(row=>row.querySelector('.task-title')?.textContent===title),title);
    await openJournal();
    const bounds=await journal.locator('#calendar').boundingBox();
    const viewport=journal.viewportSize()||await journal.evaluate(()=>({width:innerWidth,height:innerHeight}));
    assert(bounds.width>viewport.width*.9,'Month must use the full window width');
    assert(bounds.height>viewport.height*.65,'Month must use the full window height');
    assert.equal(await journal.locator('#note').isVisible(),false);
    await openDay(today);
    assert(await journal.locator('li').filter({hasText:title}).locator('input').isChecked());
    await journal.locator('li').filter({hasText:title}).locator('input').click();
    await widget.waitForFunction(title=>[...document.querySelectorAll('li:not(.done)')].some(row=>row.querySelector('.task-title')?.textContent===title),title);
    await journal.locator('#close-day').click();
    await journal.waitForFunction(()=>!document.querySelector('#day-dialog').open);
    await previousMonth();
    const dates=await journal.locator('.calendar-day').evaluateAll(buttons=>buttons.map(button=>button.dataset.date));
    for(const date of dates){
      if((await invoke(journal,'get_note',{date})).version===0&&(await invoke(journal,'get_day',{date})).length===0){noteDate=date;break;}
    }
    assert(noteDate,'Need an unused day for temporary fixtures');
    for(const text of fixtureTitles)await invoke(widget,'create_task',{date:noteDate,title:text});
    const cell=journal.locator(`.calendar-day[data-date="${noteDate}"]`);
    await journal.waitForFunction(date=>document.querySelector(`.calendar-day[data-date="${date}"] .cell-more`)?.textContent==='另 2 项',noteDate);
    assert.equal(await cell.locator('.cell-task').count(),3);
    assert.equal(await cell.locator('.cell-task').first().getAttribute('title'),fixtureTitles[0]);
    await openDay(noteDate);
    assert.equal(await journal.locator('#task-list li').count(),5);
    assert(await journal.locator('#note').evaluate(el=>el.readOnly));
    await journal.locator('#edit-day').click();
    await journal.waitForFunction(()=>!document.querySelector('#note').readOnly&&!document.querySelector('#note').disabled);
    // Entry actions remain collapsed until requested.
    const firstEntry=journal.locator('#task-list li').filter({hasText:fixtureTitles[0]});
    await firstEntry.locator('.entry-menu').click();
    const renamedTitle=fixtureTitles[0]+'（已修改）';
    await firstEntry.locator('.task-editor input').fill(renamedTitle);
    await firstEntry.getByRole('button',{name:'保存',exact:true}).click();
    await journal.waitForFunction(text=>[...document.querySelectorAll('#task-list .task-title')].some(el=>el.textContent===text),renamedTitle);
    const renamedEntry=journal.locator('#task-list li').filter({hasText:renamedTitle});
    await renamedEntry.locator('.reflection-input').fill(`事项反思 ${runId}`);
    for(let i=0;i<50;i++){if((await invoke(journal,'get_day',{date:noteDate})).find(row=>row.title===renamedTitle)?.reflection===`事项反思 ${runId}`)break;await delay(100);}
    assert.equal((await invoke(journal,'get_day',{date:noteDate})).find(row=>row.title===renamedTitle)?.reflection,`事项反思 ${runId}`);
    await journal.waitForFunction(()=>document.activeElement?.classList.contains('reflection-input'));

    await renamedEntry.getByRole('button',{name:'下移',exact:true}).click();
    await journal.waitForFunction(text=>document.querySelector('#task-list .task-title')?.textContent===text,fixtureTitles[1]);
    await renamedEntry.getByRole('button',{name:'删除',exact:true}).click();
    await journal.locator('#deleted-tasks summary').waitFor();
    await journal.locator('#deleted-tasks summary').click();
    await journal.locator('#deleted-list li').filter({hasText:renamedTitle}).getByRole('button',{name:'恢复',exact:true}).click();
    await renamedEntry.waitFor();
    assert.equal(await journal.locator('#task-list li').count(),5);
    assert.equal((await journal.locator('#task-list .task-title').allTextContents())[1],renamedTitle);
    await renamedEntry.locator('.entry-menu').click();
    await renamedEntry.locator('.task-editor input').fill(`未保存 ${runId}`);
    await journal.locator('#close-day').click();
    await journal.locator('#note-status.error').waitFor();
    assert(await journal.locator('#day-dialog').evaluate(el=>el.open));
    assert.equal(await renamedEntry.locator('.task-editor input').inputValue(),`未保存 ${runId}`);
    await renamedEntry.getByRole('button',{name:'取消',exact:true}).click();

    await widget.locator('#backlog summary').click();
    await widget.locator('.backlog-entry').filter({hasText:renamedTitle}).getByRole('button',{name:'安排到今天'}).click();
    await widget.locator('#task-list .task-title').filter({hasText:renamedTitle}).waitFor();
    const carried=(await invoke(widget,'get_day',{date:today})).find(row=>row.title===renamedTitle);
    assert(carried);assert.equal(carried.reflection,'');
    assert.equal((await invoke(widget,'get_day',{date:noteDate})).find(row=>row.title===renamedTitle).carried_to,today);
    await journal.locator('#task-input:enabled').waitFor();
    await widget.locator('#undo:enabled').click();
    await widget.locator('#task-list .task-title').filter({hasText:renamedTitle}).waitFor({state:'detached'});
    assert.equal((await invoke(widget,'get_day',{date:noteDate})).find(row=>row.title===renamedTitle).carried_to,null);
    // A dirty draft in either window must block an operation affecting all data.
    await widget.locator('#task-input:enabled').fill(`未添加 ${runId}`);
    const rejected=await invoke(journal,'undo_change').then(()=>false,()=>true);assert(rejected);
    assert.equal(await widget.locator('#task-input').inputValue(),`未添加 ${runId}`);
    await widget.locator('#task-input').fill('');
    const blocker=new DatabaseSync(dbPath);
    try{
      blocker.exec('BEGIN IMMEDIATE');
      await journal.locator('#note').fill(noteText);
      await journal.locator('#close-day').click();
      await journal.locator('#note-status.error').waitFor();
      assert(await journal.locator('#day-dialog').evaluate(el=>el.open));
      assert.equal(await journal.locator('#note').inputValue(),noteText);
    }finally{blocker.exec('ROLLBACK');blocker.close();}
    // Retry switching after failure, flushing to the original day.
    await journal.locator('#detail-today').click();await waitDay(today);
    assert.equal((await invoke(journal,'get_note',{date:noteDate})).body,noteText);
    assert.notEqual(await journal.locator('#note').inputValue(),noteText);
    await journal.locator('#close-day').click();
    await journal.waitForFunction(()=>!document.querySelector('#day-dialog').open);
    await previousMonth();
    assert((await cell.getAttribute('aria-label')).includes('有日记'));
    await journal.locator('#settings').click();
    await journal.locator('#settings-dialog[open]').waitFor();
    const startup=journal.locator('#autostart');
    assert.equal(await startup.isChecked(),false);await startup.check();
    await journal.waitForFunction(()=>!document.querySelector('#autostart').disabled&&document.querySelector('#settings-status').textContent==='已保存');
    assert.equal((await invoke(journal,'get_maintenance')).autostart,true);
    await startup.uncheck();
    await journal.waitForFunction(()=>!document.querySelector('#autostart').disabled&&document.querySelector('#settings-status').textContent==='已保存');
    assert.equal((await invoke(journal,'get_maintenance')).autostart,false);
    async function fileAction(action,file,confirm=false){
      await journal.locator(`[data-file="${action}"]`).click();
      await delay(300);
      console.log('File action state:',await journal.locator('#settings-status').textContent(),'open:',await journal.locator('#settings-dialog').evaluate(el=>el.open));
      const args=['-NoProfile','-STA','-ExecutionPolicy','Bypass','-File',path.join(root,'scripts/native-dialog.ps1'),'-AppPid',String(pid),'-FilePath',file];
      if(confirm)args.push('-ConfirmRestore');
      const result=execFileSync('powershell.exe',args,{windowsHide:true,encoding:'utf8',timeout:45000});console.log(result.trim());
      await journal.waitForFunction(()=>!document.querySelector('#close-settings').disabled);
      const message=await journal.locator('#settings-status').textContent();assert(!await journal.locator('#settings-status').evaluate(el=>el.classList.contains('error')),message);
      return message;
    }
    const exportedJson=path.join(testDirectory,'日页导出.json'),exportedMd=path.join(testDirectory,'日页导出.md'),exportedDb=path.join(testDirectory,'日页备份.sqlite3');
    await fileAction('json',exportedJson);assert.equal(JSON.parse(fs.readFileSync(exportedJson,'utf8')).version,3);
    await fileAction('markdown',exportedMd);assert(fs.readFileSync(exportedMd,'utf8').includes(`事项反思 ${runId}`));
    await fileAction('backup',exportedDb);assert(fs.existsSync(exportedDb));
    const temporary=await invoke(widget,'create_task',{date:today,title:`恢复后应消失 ${runId}`});
    await widget.locator('#task-input:enabled').waitFor();
    await fileAction('restore',exportedJson,true);
    assert(!(await invoke(widget,'get_day',{date:today})).some(row=>row.id===temporary.id));
    assert.equal((await invoke(widget,'get_note',{date:noteDate})).body,noteText);
    assert(fs.readdirSync(path.join(testDirectory,'backups')).some(name=>name.startsWith('before-restore-')));
    await fileAction('restore',exportedDb,true);
    await journal.locator('#close-settings').click();
    await widget.locator('#widget-theme').selectOption('dark');await setOpacity(.55);
    expectedLayout={main:await geometry(widget),journal:await geometry(journal)};
    expectedLayout.main={...expectedLayout.main,x:40,y:40,width:expectedLayout.main.width+20,height:expectedLayout.main.height+20};
    expectedLayout.journal={...expectedLayout.journal,x:60,y:60,width:expectedLayout.journal.width-20,height:expectedLayout.journal.height-20};
    await stop();fs.writeFileSync(layoutPath,JSON.stringify(expectedLayout));await start();
    for(let i=0;i<40;i++){if(JSON.stringify(await geometry(widget))===JSON.stringify(expectedLayout.main))break;await delay(100);}
    assert.deepEqual(await geometry(widget),expectedLayout.main,'Widget layout must restore across restart');
    assert.equal(await widget.locator('#widget-theme').inputValue(),'dark');assert.equal(Number(await opacity()),.55);
    await restoreAppearance();
    assert(!(await widget.locator('li').filter({hasText:title}).locator('input').isChecked()));
    await openJournal();
    for(let i=0;i<40;i++){if(JSON.stringify(await geometry(journal))===JSON.stringify(expectedLayout.journal))break;await delay(100);}
    assert.deepEqual(await geometry(journal),expectedLayout.journal,'Journal layout must restore');
    await previousMonth();await openDay(noteDate);
    assert.equal(await journal.locator('#note').inputValue(),noteText);
    const closed=journal.waitForEvent('close');
    await journal.evaluate(()=>{void window.__TAURI__.window.getCurrentWindow().close();});await closed;
    assert(!widget.isClosed());
    await openJournal();
    assert.deepEqual(await geometry(journal),expectedLayout.journal,'Journal layout must survive window recreation');
    await stop();cleanup();
    const savedLayout=JSON.parse(fs.readFileSync(layoutPath,'utf8'));
    assert.deepEqual(savedLayout.main,expectedLayout.main,'Exit must save widget layout');
    savedLayout.main.x=100000;savedLayout.main.y=100000;
    fs.writeFileSync(layoutPath,JSON.stringify(savedLayout));await start();
    const visible=await widget.evaluate(async()=>{const w=window.__TAURI__.window.getCurrentWindow();const p=await w.outerPosition();const m=await window.__TAURI__.window.primaryMonitor();return {p,area:m.workArea};});
    assert(visible.p.x>=visible.area.position.x&&visible.p.x<visible.area.position.x+visible.area.size.width);
    assert(visible.p.y>=visible.area.position.y&&visible.p.y<visible.area.position.y+visible.area.size.height);
    fs.mkdirSync(path.join(root,'verification'),{recursive:true});
    await setOpacity(.82);
    for(const theme of ['original','light','dark']){
      await widget.locator('#widget-theme').selectOption(theme);
      await widget.screenshot({path:path.join(root,`verification/widget-${theme}.png`),omitBackground:true});
    }
    await restoreAppearance();
    const memory=execFileSync('powershell.exe',['-NoProfile','-Command',`$tree=@(${pid}); $all=Get-CimInstance Win32_Process; do {$next=@($all | Where-Object {$_.ParentProcessId -in $tree -and $_.ProcessId -notin $tree} | ForEach-Object {[int]$_.ProcessId}); $tree+=$next} while($next.Count); $p=@(Get-Process -Id $tree -ErrorAction SilentlyContinue); [pscustomobject]@{processes=$p.Count;workingSetMiB=[math]::Round(($p | Measure-Object WorkingSet64 -Sum).Sum/1MB,1);privateMiB=[math]::Round(($p | Measure-Object PrivateMemorySize64 -Sum).Sum/1MB,1)} | ConvertTo-Json -Compress`],{windowsHide:true,encoding:'utf8'});
    fs.writeFileSync(path.join(root,'verification/memory.json'),memory);
    await openJournal();
    await journal.screenshot({path:path.join(root,'verification/month-calendar.png')});
    assert.deepEqual(errors,[]);
    passed=true;
    console.log('PASS: reflection autosave/focus, carry/undo/cross-window draft guard, autostart, native backup/JSON/Markdown/restore dialogs, bold widget text, window layout restart/recreation, title edit, ordering, soft delete and restore, draft protection, themes and opacities, month previews, safe save/retry/close, restart persistence and WebView release.');
  }catch(error){if(journal&&!journal.isClosed()) {try{await journal.screenshot({path:path.join(root,'verification/failure.png')});}catch{}}console.error('Acceptance failure:',error.message);throw error;}
  finally{try{await restoreAppearance();await stop();}finally{try{execFileSync('reg.exe',['delete','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run','/v',`DaypageAcceptance-${runId}`,'/f'],{windowsHide:true,stdio:'ignore'});}catch{}cleanup();if(originalLayout===null){fs.rmSync(layoutPath,{force:true});}else fs.writeFileSync(layoutPath,originalLayout);if(passed&&path.dirname(testDirectory)===path.join(root,"verification","test-data"))fs.rmSync(testDirectory,{recursive:true,force:true,maxRetries:20,retryDelay:200});}}
})().catch(error=>{console.error(error);process.exitCode=1;});
