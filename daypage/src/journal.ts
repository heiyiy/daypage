import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { initSettings } from "./settings";
import { localDate } from "./dates";

interface Note { body: string; version: number }
interface DaySummary {
  date: string; total: number; completed: number; has_note: boolean;
  tasks: { title: string; completed: boolean }[];
}
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export function journalShell(tasks: string) {
  return `<main class="month-layout"><header class="month-toolbar"><div class="month-navigation"><button id="prev-month" aria-label="上个月">‹</button><h1 id="month-label"></h1><button id="next-month" aria-label="下个月">›</button><button id="today">本月</button></div><div class="month-actions"><button id="settings">设置</button><button id="show-widget">桌面待办</button></div></header>
    <div class="weekdays"><span>周一</span><span>周二</span><span>周三</span><span>周四</span><span>周五</span><span>周六</span><span>周日</span></div>
    <div id="calendar" class="calendar-grid" aria-label="月历"></div><p id="calendar-status" role="status">读取中…</p></main>
    <dialog id="day-dialog" aria-labelledby="date-label"><header class="detail-toolbar"><div class="detail-navigation"><button id="prev-day" aria-label="前一天">‹</button><h2 id="date-label"></h2><button id="next-day" aria-label="后一天">›</button><button id="detail-today">今天</button></div><button id="close-day" aria-label="返回月历">返回月历</button></header>
    <div class="day-content"><section class="journal-tasks"><div class="section-heading"><h3>待办</h3><button id="edit-day" hidden>编辑这一天</button></div>${tasks}</section>
    <section class="note-section"><label for="note">日记 / 反思</label><textarea id="note" spellcheck="false" disabled></textarea><div class="note-footer"><span id="note-status" role="status" aria-live="polite">读取中…</span><button id="save-note">保存</button></div></section></div></dialog>`;
}

export async function initJournal(selectDate: (date: string, editable: boolean) => Promise<void>, assertTasksSaved:()=>void) {
  const editor = $<HTMLTextAreaElement>("note");
  const status = $("note-status");
  const dialog = $<HTMLDialogElement>("day-dialog");
  let activeDate = localDate();
  let month = new Date(`${activeDate.slice(0, 7)}-01T12:00:00`);
  let version = 0;
  let savedBody = "";
  let loaded = false;
  let switching = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let flight: Promise<boolean> | null = null;
  let calendarSequence = 0;

  function error(error: unknown) {
    status.textContent = `保存失败：${String(error)}`;
    status.className = "error";
  }
  async function refreshMonth() {
    const sequence = ++calendarSequence;
    const year = month.getFullYear(), number = month.getMonth();
    try {
      const rows = await invoke<DaySummary[]>("get_month", { year, month: number + 1 });
      if (sequence !== calendarSequence) return;
      const summaries = new Map(rows.map(row => [row.date, row]));
      $("month-label").textContent = `${year} 年 ${number + 1} 月`;
      const calendar = $("calendar"); calendar.replaceChildren();
      const offset = (new Date(year, number, 1).getDay() + 6) % 7;
      const count = new Date(year, number + 1, 0).getDate();
      const weeks = Math.ceil((offset + count) / 7);
      calendar.style.setProperty("--week-count", String(weeks));
      for (let index = 0; index < weeks * 7; index++) {
        const day = index - offset + 1;
        if (day < 1 || day > count) {
          const blank = document.createElement("div"); blank.className = "calendar-blank"; calendar.append(blank); continue;
        }
        const date = localDate(new Date(year, number, day));
        const info = summaries.get(date);
        const button = document.createElement("button"); button.type = "button";
        button.className = `calendar-day ${dialog.open && date === activeDate ? "selected" : ""} ${date === localDate() ? "is-today" : ""}`;
        button.dataset.date = date;
        button.setAttribute("aria-label", `${date}${info?.has_note ? "，有日记" : ""}`);
        const heading = document.createElement("span"); heading.className = "cell-heading";
        const label = document.createElement("span"); label.className = "day-number"; label.textContent = String(day);
        const meta = document.createElement("small");
        meta.textContent = `${info?.total ? `${info.completed}/${info.total}` : ""}${info?.has_note ? " 日记" : ""}`;
        heading.append(label, meta); button.append(heading);
        const previews = document.createElement("span"); previews.className = "cell-tasks";
        for (const task of info?.tasks ?? []) {
          const preview = document.createElement("span"); preview.className = task.completed ? "cell-task completed" : "cell-task";
          preview.textContent = `${task.completed ? "✓" : "·"} ${task.title}`; preview.title = task.title;
          previews.append(preview);
        }
        const remaining = (info?.total ?? 0) - (info?.tasks.length ?? 0);
        if (remaining > 0) {
          const more = document.createElement("small"); more.className = "cell-more"; more.textContent = `另 ${remaining} 项`; previews.append(more);
        }
        button.append(previews);
        button.addEventListener("click", () => { void changeDay(date); });
        calendar.append(button);
      }
      $("calendar-status").textContent = "点击日期展开待办与反思";
    } catch (reason) { if (sequence === calendarSequence) $("calendar-status").textContent = `读取失败：${String(reason)}`; }
  }
  async function flush(): Promise<boolean> {
    if (timer) { clearTimeout(timer); timer = undefined; }
    if (flight) return (await flight) && flush();
    if (!loaded || editor.value === savedBody) return true;
    const date = activeDate, body = editor.value, expectedVersion = version;
    status.className = ""; status.textContent = "保存中…";
    flight = (async () => {
      try {
        version = await invoke<number>("save_note", { date, body, version: expectedVersion });
        savedBody = body;
        status.textContent = editor.value === body ? "已保存" : "待保存";
        return true;
      } catch (reason) { error(reason); return false; }
      finally { flight = null; }
    })();
    const ok = await flight;
    return ok && flush();
  }
  async function changeDay(date: string, forceEdit = false) {
    if (switching) return;
    switching = true;
    editor.disabled = true;
    try {
      if (!await flush()) return;
      const note = await invoke<Note>("get_note", { date });
      const editable = forceEdit || date === localDate();
      await selectDate(date, editable);
      activeDate = date; version = note.version; savedBody = note.body; loaded = true;
      editor.value = note.body; editor.readOnly = !editable;
      $<HTMLButtonElement>("save-note").disabled = !editable;
      $("edit-day").hidden = editable;
      status.textContent = editable ? "本地已保存" : "只读"; status.className = "";
      month = new Date(`${date.slice(0, 7)}-01T12:00:00`);
      if (!dialog.open) dialog.showModal();
      await refreshMonth();
    } catch (reason) {
      if (dialog.open) error(reason);
      else $("calendar-status").textContent = `打开失败：${String(reason)}`;
    } finally { switching = false; editor.disabled = !loaded; }
  }
  async function finish(action: () => Promise<void> | void) {
    if (switching) return false;
    switching = true; editor.disabled = true;
    try {
      assertTasksSaved();
      if (!await flush()) return false;
      await action();
      return true;
    }
    catch (reason) { error(reason); return false; }
    finally { switching = false; editor.disabled = !loaded; }
  }
  async function closeDay() {
    await finish(async () => { dialog.close(); await refreshMonth(); });
  }
  function adjacentDay(offset: number) {
    const next = new Date(`${activeDate}T12:00:00`); next.setDate(next.getDate() + offset);
    void changeDay(localDate(next));
  }
  editor.addEventListener("input", () => {
    status.textContent = "待保存"; status.className = "";
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => { void flush(); }, 500);
  });
  $("save-note").addEventListener("click", () => { void flush(); });
  $("edit-day").addEventListener("click", () => { void changeDay(activeDate, true); });
  $("detail-today").addEventListener("click", () => { void changeDay(localDate()); });
  $("prev-day").addEventListener("click", () => { adjacentDay(-1); });
  $("next-day").addEventListener("click", () => { adjacentDay(1); });
  $("close-day").addEventListener("click", () => { void closeDay(); });
  dialog.addEventListener("cancel", event => { event.preventDefault(); void closeDay(); });
  $("today").addEventListener("click", () => { month = new Date(`${localDate().slice(0, 7)}-01T12:00:00`); void refreshMonth(); });
  $("prev-month").addEventListener("click", () => { month = new Date(month.getFullYear(), month.getMonth() - 1, 1); void refreshMonth(); });
  $("next-month").addEventListener("click", () => { month = new Date(month.getFullYear(), month.getMonth() + 1, 1); void refreshMonth(); });
  $("show-widget").addEventListener("click", () => { void invoke("show_widget").catch(error); });
  await listen("tasks-changed", () => { void refreshMonth(); });
  await listen("notes-changed", () => { void refreshMonth(); });
  await listen("data-replaced",()=>{
    void refreshMonth();
    if(!dialog.open){loaded=false;editor.value="";savedBody="";return;}
    if(loaded && !flight && editor.value===savedBody) void changeDay(activeDate,!editor.readOnly);
    else if(loaded)error("数据已变更，当前草稿已保留；请复制草稿后重新打开这一天");
  });
  initSettings(()=>finish(()=>{}));
  await listen("request-exit", async () => {
    if (!await finish(async () => { await invoke("quit_app"); })) await invoke("open_journal");
  });
  await getCurrentWindow().onCloseRequested(async event => {
    event.preventDefault();
    await finish(async () => { await getCurrentWindow().destroy(); });
  });
  window.addEventListener("focus", () => { void refreshMonth(); });
  await refreshMonth();
  await listen("finish-data-change",()=>{editor.disabled=!loaded;});
  return async()=>{
    if(!await finish(()=>{}))throw new Error(status.textContent??"请先保存日记");
    editor.disabled=true;
  };
}
