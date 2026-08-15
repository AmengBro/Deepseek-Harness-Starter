# DeepseekHarness v1.0.1 发行说明

首个开源发布（[GPL-3.0](LICENSE)）。DeepseekHarness 是一个基于 Tauri 的轻量级桌面启动器：自动检测 Node.js、一键启动 DeepSeek Harness 本地 Web 服务，并在内嵌 WebView 中展示服务界面。

## ✨ 功能特性

- 一键启动 `npx @deepseek-ai/dsh web`，服务日志实时显示在主界面
- 智能依赖管理：`npx` 超过 10 秒未就绪时自动切换为 `npm install -g @deepseek-ai/dsh --verbose`，下载全过程实时反馈，避免误以为卡死
- Node.js 自动检测（多策略定位 `node.exe`）
- 内嵌 WebView 展示服务界面（默认端口 3080，占用时自动 +1）
- 系统托盘常驻：右键菜单「打开主界面 / 设置 / 退出」
- 设置窗口：服务端口、打开日志文件夹、打开安装目录，关闭自动保存
- 日志按日期滚动（`applog` / `npmlog`），保留最近 30 天
- 服务崩溃自动重启（最多 3 次，间隔 5 秒）
- 基于 GitHub Releases 的自动更新检查
- MewUI 风格安装器（Direct2D + Mica 材质，样式参考 Starward.Setup），按用户安装、免管理员，支持 `/S` 静默安装与 `uninstall /S` 静默卸载

## 🐛 修复

- 修复托盘菜单「设置」无法打开设置窗口的问题
- 修复主界面误报「找不到 Node.js」的问题（前端命令注册链路未接通导致）
- 修复服务启动流程偶发卡死
- 修复端口上已有 dsh 服务时仍重复启动新服务的问题（现在会直接复用并显示现有服务）
- 更换与 DeepSeek 官方近似的 bundle 标识符，避免商标混淆

## 🧰 安装包

- **MewUI 安装器**（推荐）：`DeepseekHarness.Setup-1.0.1-win-x64.exe`（单文件自包含，免管理员）
- **NSIS 安装器**：`DeepseekHarness_1.0.1_x64-setup.exe`（Tauri 官方打包，体积更小）

## 系统要求

- Windows 10 1809（build 17763）及以上；macOS / Linux 支持开发运行（安装器目前仅提供 Windows x64）
- Node.js 18+（未安装时应用会引导前往 nodejs.org 下载）

## 使用提示

- 首次启动服务时会自动下载 dsh 依赖，界面会实时显示 npm 详细日志，请耐心等待完成
- 配置与日志保存在应用安装目录下的 `config/` 文件夹，不写入注册表或 AppData
- 卸载时可选择保留 `config/`（端口配置与日志）

## 已知限制

- Node.js 未安装时暂仅提供官网下载引导，aria2 自动静默安装将在后续版本提供
- 自动更新依赖 GitHub Releases 发布（使用本 tag 对应的 Release）

## 第三方许可

第三方组件及其许可证清单见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
