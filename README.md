# 日页 · Daypage

A lightweight software for creating to-do lists and summarizing daily reflections

简洁的 Windows 本地待办、事项反思与日记工具。透明桌面小组件用于记录待办，独立月历用于翻看每天的安排与日记。数据保存在本机，日常使用无需联网。

## 功能

- 透明待办小组件：三种配色、三档透明度、加粗文字与托盘，只在桌面显示，切换到应用时自动隐藏。
- 待办编辑、排序、完成、删除恢复、跨天安排与事项反思。
- 整月日历预览，展开日期查看完整待办及日记，支持自动保存。
- 持久化撤销、自动与手动备份、SQLite / JSON 恢复、JSON / Markdown 导出。
- 可选开机启动，记住窗口位置和大小。

## 开发

技术栈：Tauri 2、TypeScript、Vite、Rust、SQLite。Windows 构建需要 Node.js、Rust、Microsoft C++ Build Tools 与 WebView2。

```powershell
cd daypage
npm.cmd ci
npm.cmd run tauri dev
```

## 构建和测试

在 `daypage` 目录运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
npm.cmd run tauri build -- --bundles nsis
```

可执行文件生成于 `daypage/src-tauri/target/release/daypage.exe`，安装包生成于其下的 `bundle/nsis/`。

详细操作、备份及验收说明见 [使用说明](daypage/README.md) 和 [验收记录](daypage/verification/验收记录.md)。

