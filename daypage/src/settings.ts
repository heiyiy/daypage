import { invoke } from "@tauri-apps/api/core";

interface Maintenance {autostart:boolean;backup_directory:string;backup_error:string;undo:string|null}
export function initSettings(prepare:()=>Promise<boolean>) {
  const dialog=document.createElement("dialog");dialog.id="settings-dialog";
  dialog.innerHTML=`<header><strong>设置与数据</strong><button id="close-settings" aria-label="关闭设置">×</button></header>
    <label class="startup-option"><input type="checkbox" id="autostart">开机启动</label>
    <p>自动备份：保留最近 30 天，每次保存后更新。</p><p id="backup-directory"></p>
    <div class="settings-actions"><button data-file="directory">更改备份目录</button><button data-file="backup">立即备份</button><button data-file="restore">恢复备份 / JSON</button><button data-file="json">导出 JSON</button><button data-file="markdown">导出 Markdown</button></div>
    <p id="settings-status" role="status"></p>`;
  document.body.append(dialog);
  const status=dialog.querySelector<HTMLElement>("#settings-status")!;
  const startup=dialog.querySelector<HTMLInputElement>("#autostart")!;
  let busy=false;
  const error=(reason:unknown)=>{status.textContent=String(reason);status.className="error";};
  async function refresh() {
    const data=await invoke<Maintenance>("get_maintenance");startup.checked=data.autostart;
    dialog.querySelector("#backup-directory")!.textContent=data.backup_directory;
    if(data.backup_error)error(`自动备份失败：${data.backup_error}`);
  }
  document.getElementById("settings")!.addEventListener("click",async()=>{
    if(!await prepare())return;
    status.textContent="";status.className="";dialog.showModal();try{await refresh();}catch(reason){error(reason);}
  });
  dialog.querySelector("#close-settings")!.addEventListener("click",()=>{if(!busy)dialog.close();});
  dialog.addEventListener("cancel",event=>{if(busy)event.preventDefault();});
  async function run(action:()=>Promise<void>) {
    if(busy)return;busy=true;dialog.querySelectorAll<HTMLButtonElement|HTMLInputElement>("button,input").forEach(el=>el.disabled=true);
    status.className="";status.textContent="处理中…";
    try{await action();}catch(reason){error(reason);}finally{busy=false;dialog.querySelectorAll<HTMLButtonElement|HTMLInputElement>("button,input").forEach(el=>el.disabled=false);}
  }
  startup.addEventListener("change",()=>{const enabled=startup.checked;void run(async()=>{try{await invoke("set_autostart",{enabled});status.textContent="已保存";}finally{await refresh();}});});
  dialog.querySelectorAll<HTMLButtonElement>("[data-file]").forEach(button=>button.addEventListener("click",()=>{void run(async()=>{
    const result=await invoke<string|null>("file_action",{action:button.dataset.file});
    status.textContent=result?`完成：${result}`:"已取消";await refresh();
  });}));
}
