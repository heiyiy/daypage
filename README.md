# 日页 · Daypage

一个简洁的 Windows 本地待办与日记工具。

在桌面上写待办，打开月历翻看每天的安排、事项反思和日记。没有账号和云端同步，数据保存在自己的电脑上。

## 下载

**[下载最新版本](https://github.com/heiyiy/daypage/releases/latest)** · 当前版本 **v0.2.1** · Windows x64

| 文件 | 适合谁 | 下载 |
| --- | --- | --- |
| 安装版 | 日常使用，安装后从开始菜单打开 | [下载安装包](https://github.com/heiyiy/daypage/releases/download/v0.2.1/Daypage-0.2.1-windows-x64-setup.exe) |
| 便携版 | 不想安装，双击即可运行 | [下载 exe](https://github.com/heiyiy/daypage/releases/download/v0.2.1/Daypage-0.2.1-windows-x64.exe) |
| 校验文件 | 核对下载文件的 SHA256 | [SHA256SUMS.txt](https://github.com/heiyiy/daypage/releases/download/v0.2.1/SHA256SUMS.txt) |

使用时不需要 Node.js、Rust 或终端。程序依赖 Microsoft Edge WebView2；电脑缺少该组件时，安装版会联网补装，便携版需要先安装 WebView2 Runtime。日常记录可以离线使用。

Release 下的 `Source code` 是源码压缩包；直接使用请选择上面的 exe 附件。

v0.2.1 修复了切回桌面偶尔不显示的问题：无需额外点击桌面或从日历重新调出组件。

## 能做什么

- **桌面待办**：只在 Windows 桌面显示，切换到其他应用时自动隐藏，回到桌面时恢复；支持拖动、缩放和托盘隐藏。
- **简洁外观**：原色、白底黑字、黑底白字，三档背景透明度，待办文字加粗。
- **事项管理**：新增、编辑、排序、完成、删除与恢复；每件事都可以写反思，未完成事项可以安排到今天。
- **日历与日记**：一个月一页，日期格预览当天待办，展开查看完整安排、事项反思和日记；历史内容默认只读，可以主动编辑。
- **保存与撤销**：日记、事项反思自动保存，最近 50 步已保存的数据操作可以撤销，重启后仍可用。
- **备份与导出**：每日自动备份、手动备份，支持 SQLite / JSON 恢复，以及 JSON / Markdown 导出。

## 开始使用

1. 安装或运行程序，回到 Windows 桌面即可看到待办组件。
2. 输入事项后按 `Enter` 添加，勾选表示完成；右侧 `⋯` 中可以编辑、排序、删除和写事项反思。
3. 点击“日历”，打开独立页面，查看其他日期或写当天日记。
4. 在日历右上角“设置”中配置开机启动、备份目录、恢复和导出。

打开其他应用（包括日页的日历窗口）时，待办组件会隐藏；回到桌面或按 `Win+D` 时重新显示。自动隐藏不会清空正在输入的文字。

组件的 `×` 是手动隐藏，此后回到桌面也不会自动出现。从系统托盘右键菜单选择“待办”，可以重新启用桌面显示；选择“退出”才会结束程序。

## 数据、备份与升级

数据与程序文件分开保存，默认目录为：

```text
%APPDATA%\com.personal.daypage\
├─ daypage.sqlite3      待办、事项反思、日记与设置
├─ window-layout.json  窗口位置和大小
└─ backups\            自动与手动备份
```

自动备份每天一份，每次成功保存后更新当天快照，保留最近 30 份。可以在设置中把备份目录改到其他磁盘，或导出 JSON 保存完整记录。

升级前请先从托盘退出旧版本，再运行新版安装包或替换便携版 exe。升级会保留已有数据，数据库迁移和恢复备份前会创建快照。默认卸载保留数据；如果主动勾选删除应用数据，请先备份。

便携版开启开机启动后，请保持 exe 的路径稳定；移动文件后重新关闭、开启一次开机启动。

## 从源码运行

技术栈：**Tauri 2 + TypeScript + Vite + Rust + SQLite**。

Windows 开发环境需要 Node.js（满足 Vite 8 要求，例如 Node.js 22.12+）、Rust stable、Microsoft C++ Build Tools（含 Windows SDK）和 WebView2。

```powershell
git clone https://github.com/heiyiy/daypage.git
cd daypage/daypage
npm.cmd ci
npm.cmd run tauri dev
```

构建安装包与运行 Rust 测试：

```powershell
npm.cmd run tauri build -- --bundles nsis
cargo test --manifest-path src-tauri/Cargo.toml
```

构建产物位于应用目录下的 `src-tauri/target/release/`，安装包位于其下的 `bundle/nsis/`。首次构建需要联网下载依赖。

## 更多说明

- [详细使用与备份说明](daypage/README.md)
- [测试与验收记录](daypage/verification/验收记录.md)
- [反馈问题](https://github.com/heiyiy/daypage/issues)

当前主要面向 Windows x64。真实多屏热插拔、休眠恢复及不同设备上的输入法体验仍需持续验证。
