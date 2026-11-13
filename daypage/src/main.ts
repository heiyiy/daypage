import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { localDate, dateLabel } from "./dates";
import { journalShell, initJournal } from "./journal";

interface DayTask { id: string; title: string; completed: boolean; version: number; reflection: string; carried_to: string | null; date: string }
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const isJournal = new URLSearchParams(location.search).get("view") === "journal";
document.title = isJournal ? "日页 · 日记与日历" : "日页 · 待办";
document.body.className = isJournal ? "journal" : "widget";
const taskMarkup = `<details id="backlog" hidden><summary>此前未完成 <span id="backlog-count"></span></summary><ul id="backlog-list"></ul></details><form id="add-form"><input id="task-input" aria-label="新待办" placeholder="添加待办，回车保存" maxlength="500" autocomplete="off" disabled /><button id="add-button" type="submit" disabled title="添加待办">＋</button></form>
  <ul id="task-list" aria-label="任务列表"></ul><p id="empty-state">暂无待办</p>
  <details id="deleted-tasks" hidden><summary id="deleted-count"></summary><ul id="deleted-list"></ul></details>
  <footer><span id="save-status" role="status" aria-live="polite">读取中…</span><button id="retry" type="button" hidden>重试</button><button id="undo" type="button" disabled>撤销</button><span id="progress"></span></footer><p id="backup-warning" class="error" hidden></p>`;
$("app").innerHTML = isJournal ? journalShell(taskMarkup) : `<section class="widget-panel">
  <header class="widget-header"><div class="drag-handle" data-tauri-drag-region><strong data-tauri-drag-region>待办</strong><span id="date-label" data-tauri-drag-region></span></div>
  <nav><select id="widget-theme" aria-label="待办配色" title="待办配色"><option value="original">原色</option><option value="light">白底黑字</option><option value="dark">黑底白字</option></select><button id="opacity" title="切换透明度" aria-label="切换透明度">◐</button><button id="open-journal" title="日记 / 日历">日历</button><button id="hide" title="隐藏到托盘" aria-label="隐藏到托盘">×</button></nav></header>
  <div class="task-area">${taskMarkup}</div><p id="day-notice" hidden>日期已变更 <button id="go-today">回到今天</button></p></section>`;
const input = $<HTMLInputElement>("task-input");
const add = $<HTMLButtonElement>("add-button");
const list = $("task-list");
const status = $("save-status");
const retry = $("retry");
let tasks: DayTask[] = [];
let deleted: DayTask[] = [];
let backlog: DayTask[] = [];
let undoLabel: string | null = null;
const reflectionDrafts = new Map<string,string>();
const reflectionTimers = new Map<string,ReturnType<typeof setTimeout>>();
let expandedTaskId: string | null = null;
const titleDrafts = new Map<string,string>();
let date = localDate();
let ready = false;
let busy = false;
let maintenanceBusy=false;
let prepareJournal:()=>Promise<void>=async()=>{};
let rememberedFocus:{id:string;kind:string;start:number;end:number}|null=null;
function rememberFocus(){const field=document.activeElement as HTMLInputElement;if(field?.dataset.editId)rememberedFocus={id:field.dataset.editId,kind:field.dataset.editKind??"title",start:field.selectionStart??0,end:field.selectionEnd??0};}
let pendingRefresh = false;
let readOnly = false;
let composing = false;
let compositionEnd = -Infinity;

function showDate() {
  void invoke("check_backup").catch(fail);
  $("date-label").textContent = dateLabel(date);
  $("day-notice")?.toggleAttribute("hidden", date === localDate());
  input.placeholder = `添加到 ${date.slice(5).replace("-", "/")}，回车保存`;
}
function controls() {
  input.disabled = !ready || busy || maintenanceBusy || readOnly;
  add.disabled = !ready || busy || maintenanceBusy || readOnly;
  $("add-form").hidden = readOnly;
  retry.toggleAttribute("disabled", busy);
  $<HTMLButtonElement>("undo").disabled = busy || maintenanceBusy || !ready || readOnly || !undoLabel;
  list.querySelectorAll<HTMLInputElement>("input,textarea").forEach(el => { el.disabled = busy || maintenanceBusy || readOnly; });
  document.querySelectorAll<HTMLButtonElement>(".entry-action").forEach(el => { el.disabled = busy || maintenanceBusy || readOnly || el.dataset.boundary === "true"; });
}
function render() {
  const focused = document.activeElement as HTMLInputElement | null;
  const focusId = rememberedFocus?.id ?? focused?.dataset.editId;
  const focusKind = rememberedFocus?.kind ?? focused?.dataset.editKind ?? "title";
  const selection = focusId ? [rememberedFocus?.start ?? focused?.selectionStart ?? 0, rememberedFocus?.end ?? focused?.selectionEnd ?? 0] : null;
  rememberedFocus=null;
  list.replaceChildren();
  for (const [index, task] of tasks.entries()) {
    const row = document.createElement("li");
    row.className = task.completed ? "task done" : "task";
    const label = document.createElement("label");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = task.completed;
    const title = document.createElement("span");
    title.className = "task-title";
    title.textContent = task.title;
    label.append(checkbox, title);
    const line = document.createElement("div"); line.className = "task-line";
    line.append(label);
    const menu = document.createElement("button"); menu.type = "button"; menu.className = "entry-menu entry-action";
    menu.textContent = "⋯"; menu.setAttribute("aria-label", `操作：${task.title}`); menu.setAttribute("aria-expanded", String(expandedTaskId === task.id)); menu.hidden = readOnly;
    menu.addEventListener("click", () => { expandedTaskId = expandedTaskId === task.id ? null : task.id; render(); });
    line.append(menu); row.append(line);
    if (task.carried_to) {
      const info=document.createElement("small"); info.className="carry-info";info.textContent=`已结转至 ${task.carried_to}`;row.append(info);
    }
    if (task.reflection && (readOnly || expandedTaskId!==task.id) && isJournal) {
      const reflection=document.createElement("p");reflection.className="reflection-preview";reflection.textContent=task.reflection;row.append(reflection);
    }
    if (expandedTaskId === task.id && !readOnly) {
      const editor = document.createElement("div"); editor.className = "task-editor";
      const field = document.createElement("input"); field.type = "text"; field.maxLength = 500;
      field.id = `title-${task.id}`; field.dataset.editId = task.id; field.setAttribute("aria-label", "修改待办标题");
      field.value = titleDrafts.get(task.id) ?? task.title;
      field.addEventListener("input", () => { titleDrafts.set(task.id, field.value); });
      editor.append(field);
      const actions = document.createElement("div"); actions.className = "entry-actions";
      const button = (text:string, action:()=>void, boundary=false) => {
        const element=document.createElement("button"); element.type="button"; element.className="entry-action"; element.textContent=text; element.dataset.boundary=String(boundary); element.addEventListener("click",action); actions.append(element);
      };
      const save = () => { void edit(task,"rename",field.value); };
      let editingComposition = false, editingCompositionEnd = -Infinity;
      field.addEventListener("compositionstart",()=>{editingComposition=true;});
      field.addEventListener("compositionend",()=>{editingComposition=false;editingCompositionEnd=performance.now();});
      field.addEventListener("keydown", event => {
        if (event.key === "Enter") {
          event.preventDefault();
          if (!editingComposition && !event.isComposing && event.keyCode !== 229 && performance.now()-editingCompositionEnd>=80) save();
        }
      });
      button("保存",save);
      button("取消",()=>{titleDrafts.delete(task.id);reflectionDrafts.delete(task.id);clearTimeout(reflectionTimers.get(task.id));expandedTaskId=null;render();});
      button("上移",()=>{void edit(task,"up");},index===0);
      button("下移",()=>{void edit(task,"down");},index===tasks.length-1);
      button("删除",()=>{void edit(task,"delete");});
      const reflection=document.createElement("textarea");reflection.id=`reflection-${task.id}`;reflection.className="reflection-input";
      reflection.dataset.editId=task.id;reflection.dataset.editKind="reflection";reflection.maxLength=10000;reflection.setAttribute("aria-label","事项反思");reflection.placeholder="事项反思（自动保存）";
      reflection.value=reflectionDrafts.get(task.id)??task.reflection;
      const saveReflection=()=>{const current=tasks.find(item=>item.id===task.id);if(current&&reflectionDrafts.has(task.id))void edit(current,"reflection",reflectionDrafts.get(task.id));};
      const schedule=()=>{clearTimeout(reflectionTimers.get(task.id));reflectionTimers.set(task.id,setTimeout(()=>{reflectionTimers.delete(task.id);if(busy)schedule();else saveReflection();},500));};
      reflection.addEventListener("input",()=>{reflectionDrafts.set(task.id,reflection.value);schedule();});
      editor.append(actions,reflection);
      const saveReflectionButton=document.createElement("button");saveReflectionButton.type="button";saveReflectionButton.className="entry-action save-reflection";saveReflectionButton.textContent="保存反思";saveReflectionButton.addEventListener("click",saveReflection);editor.append(saveReflectionButton);
      row.append(editor);
    }
    checkbox.addEventListener("change", () => {
      const completed = checkbox.checked;
      checkbox.checked = task.completed;
      void mutate(async () => {
        await invoke("set_completed", { id: task.id, completed, version: task.version });
        task.completed = completed;
        task.version += 1;
      });
    });
    list.append(row);
  }
  const deletedList = $("deleted-list"); deletedList.replaceChildren();
  $("deleted-tasks").hidden = deleted.length === 0;
  $("deleted-count").textContent = `已删除（${deleted.length}）`;
  for (const task of deleted) {
    const row=document.createElement("li"); row.className="deleted-entry";
    const title=document.createElement("span"); title.textContent=task.title;
    const restore=document.createElement("button"); restore.type="button"; restore.textContent="恢复"; restore.className="entry-action";
    restore.addEventListener("click",()=>{void edit(task,"restore");});
    row.append(title,restore); deletedList.append(row);
  }
  $("backlog").hidden = date!==localDate() || backlog.length===0;
  $("backlog-count").textContent=String(backlog.length);
  const backlogList=$("backlog-list");backlogList.replaceChildren();
  for (const task of backlog) {
    const row=document.createElement("li");row.className="backlog-entry";
    const title=document.createElement("span");title.textContent=`${task.date.slice(5)} · ${task.title}`;
    const carry=document.createElement("button");carry.type="button";carry.className="entry-action";carry.textContent="安排到今天";
    carry.addEventListener("click",()=>{void mutate(async()=>{await invoke("carry_task",{id:task.id,version:task.version});await readTasks();});});
    row.append(title,carry);backlogList.append(row);
  }
  $<HTMLButtonElement>("undo").title=undoLabel?`撤销最近操作：${undoLabel}（所有窗口）`:"暂无可撤销操作";
  const done = tasks.filter(task => task.completed).length;
  $("progress").textContent = tasks.length ? `${done}/${tasks.length}` : "";
  $("empty-state").hidden = tasks.length > 0;
  controls();
  if (focusId && selection) {
    const field=document.getElementById(`${focusKind}-${focusId}`) as HTMLInputElement | null;
    if (field && !field.disabled) { field.focus({preventScroll:true}); field.setSelectionRange(selection[0],selection[1]); }
  }
}
async function edit(task:DayTask, action:string, title?:string) {
  await mutate(async()=>{
    tasks=await invoke<DayTask[]>("edit_entry",{id:task.id,version:task.version,action,title:title??null});
    if (action==="rename"||action==="delete") titleDrafts.delete(task.id);
    if (action==="reflection"||action==="delete") {reflectionDrafts.delete(task.id);clearTimeout(reflectionTimers.get(task.id));}
    if (action==="delete") expandedTaskId=null;
    deleted=await invoke<DayTask[]>("get_deleted",{date});
  });
}
function fail(error: unknown) {
  status.textContent = `失败：${String(error)}`;
  status.className = "error";
  retry.hidden = false;
}
async function readTasks() {
  const meta=await invoke<{undo:string|null;backup_error:string}>("get_task_meta");
  [tasks,deleted,backlog]=await Promise.all([invoke<DayTask[]>("get_day",{date}),invoke<DayTask[]>("get_deleted",{date}),invoke<DayTask[]>("get_backlog",{date})]);
  undoLabel=meta.undo;
  $("backup-warning").hidden=!meta.backup_error;$("backup-warning").textContent=meta.backup_error?`本地数据已保存；自动备份失败：${meta.backup_error}`:"";
}
async function load() {
  if (busy) { pendingRefresh = true; return; }
  rememberFocus();
  busy = true;
  controls();
  try {
    await readTasks();
    ready = true;
    status.textContent = "本地已保存";
    status.className = "";
    retry.hidden = true;
  } catch (error) { fail(error); }
  finally {
    busy = false; render();
    if (pendingRefresh) { pendingRefresh = false; void load(); }
  }
}
async function mutate(action: () => Promise<void>) {
  if (busy || maintenanceBusy || !ready || readOnly) return;
  rememberFocus();
  busy = true;
  controls();
  status.textContent = "保存中…";
  status.className = "";
  try {
    await action();
    status.textContent = "已保存";
    retry.hidden = true;
  } catch (error) { fail(error); }
  finally {
    busy = false; render(); if (!expandedTaskId) input.focus();
    if (pendingRefresh) { pendingRefresh = false; void load(); }
  }
}
input.addEventListener("compositionstart", () => { composing = true; });
input.addEventListener("compositionend", () => { composing = false; compositionEnd = performance.now(); });
input.addEventListener("keydown", event => {
  if (event.key === "Enter" && (event.isComposing || composing || event.keyCode === 229 || performance.now() - compositionEnd < 80)) event.preventDefault();
});
$("add-form").addEventListener("submit", event => {
  event.preventDefault();
  if (composing || performance.now() - compositionEnd < 80 || !input.value.trim()) return;
  const title = input.value;
  void mutate(async () => {
    const task = await invoke<DayTask>("create_task", { date, title });
    tasks.push(task);
    input.value = "";
  });
});
retry.addEventListener("click", () => { void load(); });
$("undo").addEventListener("click",()=>{
  try{assertTasksSaved();}catch(error){fail(error);return;}
  void invoke("undo_change").catch(fail);
});

async function selectDate(next: string, editable: boolean) {
  // Do not move the list to a different day while a write is still in flight.
  assertTasksSaved();
  date = next;
  readOnly = !editable;
  ready = false;
  tasks = [];
  deleted = []; backlog=[]; expandedTaskId=null; titleDrafts.clear();reflectionDrafts.clear();
  showDate();
  render();
  await load();
}
function assertTasksSaved() {
  if (busy) throw new Error("待办正在保存，请稍后操作");
  if (reflectionDrafts.size) throw new Error("请先等待事项反思保存，失败时可点击保存反思重试或取消");
  if ([...tasks,...deleted].some(task=>titleDrafts.has(task.id)&&titleDrafts.get(task.id)!==task.title)) throw new Error("请先保存或取消待办标题修改");
}

async function init() {
  await listen<string>("prepare-data-change",async event=>{
    let error:string|null=null;
    try {
      assertTasksSaved();
      if(input.value.trim())throw new Error("请先添加或清空新待办输入，再执行恢复或撤销");
      maintenanceBusy=true;controls();
      await prepareJournal();
    }catch(reason){error=String(reason);fail(reason);}
    await invoke("maintenance_ack",{token:event.payload,error});
  });
  await listen("finish-data-change",()=>{maintenanceBusy=false;controls();});
  await listen("data-replaced",()=>{if(!isJournal)void load();});
  await listen("notes-changed",()=>{void invoke<{undo:string|null}>("get_task_meta").then(meta=>{undoLabel=meta.undo;$<HTMLButtonElement>("undo").title=undoLabel?`撤销最近操作：${undoLabel}（所有窗口）`:"暂无可撤销操作";controls();}).catch(fail);});
  await listen("tasks-changed", () => {
    if (!isJournal || $<HTMLDialogElement>("day-dialog").open) void load();
  });
  if (isJournal) {
    prepareJournal=await initJournal(selectDate,assertTasksSaved);
  } else {
    showDate();
    await load();
    const window = getCurrentWindow();
    $("hide").addEventListener("click", () => { void invoke("hide_widget").catch(fail); });
    $("open-journal").addEventListener("click", () => { void invoke("open_journal").catch(fail); });
    // These are display preferences only; task data lives in SQLite.
    const theme = $<HTMLSelectElement>("widget-theme");
    const storedTheme = localStorage.getItem("widget-theme");
    theme.value = storedTheme && ["original", "light", "dark"].includes(storedTheme) ? storedTheme : "original";
    document.body.dataset.theme = theme.value;
    theme.addEventListener("change", () => {
      document.body.dataset.theme = theme.value;
      localStorage.setItem("widget-theme", theme.value);
    });
    let opacity = Number(localStorage.getItem("widget-opacity") ?? 0);
    if (![0, 1, 2].includes(opacity)) opacity = 0;
    const applyOpacity = () => document.documentElement.style.setProperty("--panel-opacity", String([.82, .55, .25][opacity]));
    applyOpacity();
    $("opacity").addEventListener("click", () => {
      opacity = (opacity + 1) % 3; applyOpacity(); localStorage.setItem("widget-opacity", String(opacity));
    });
    $("go-today").addEventListener("click", () => { void selectDate(localDate(), true).catch(fail); });
    await window.onCloseRequested(async event => { event.preventDefault(); await invoke("hide_widget"); });
  }
}
window.addEventListener("focus", showDate);
document.addEventListener("visibilitychange", () => { if (!document.hidden) showDate(); });
function watchMidnight() {
  const next = new Date(); next.setHours(24, 0, 1, 0);
  setTimeout(() => { showDate(); watchMidnight(); }, next.getTime() - Date.now());
}
watchMidnight();
void init().catch(fail);
