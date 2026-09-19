# DeepseekHarness 发行说明

## v1.3.0

> 说明：v1.2.1 未对外发布（线上最新 Release 为 v1.2.0），本次 1.3.0 一并包含 v1.2.1 的修复内容。

### ✨ 全新界面（WinUI 3 / Fluent Design）
- **视觉体系重构**：全面采用 WinUI 3 设计令牌（取自 WinUIonWeb 项目），强调色改为 Fluent 系统蓝（浅色 `#0067C0` / 深色 `#4CC2FF`）
- **Mica / Acrylic 材质**：应用底层为 Mica（壁纸渐变 + 半透明 + `blur(60px)`），卡片为 Acrylic 毛玻璃，窗口层次更立体
- **控件规范统一**：圆角（控件 4px / 卡片 8px / 胶囊 999px）、控件高度 32px、动效 `0.167s` / `0.2s` + `cubic-bezier(0,0,0,1)`
- **设置页改版**：侧栏改为 WinUI NavigationView 选中指示（左侧 3px 强调色竖条）；输入框聚焦改为 WinUI TextBox 底部强调条；提示条改为 InfoBar 风格（左侧状态色条）
- **启动日志页**：终端日志框改为亚克力深色面板，进度指示器改为 WinUI ProgressRing 风格

### 🪟 窗口与托盘
- **改用系统原生标题栏**：移除自绘标题栏（最小化 / 最大化 / 关闭），窗口控制与拖拽交由系统处理
- **托盘直达 dsh 界面**：dsh 就绪后，点击托盘菜单「打开主界面」或托盘图标会**直接打开 / 聚焦 dsh 窗口**，不再弹出启动日志窗口；dsh 未就绪时才显示主窗口

### 🌐 网络（面向国内用户）
- **npm 源自动切换**：启动时检测，若「npm 为官方源」且「处于国内网络」，自动将 npm 源切换为国内镜像 `npmmirror`（实测比官方源快 6–7 倍）。已配镜像或身在境外的用户不受任何影响
- **设置页可查看与手动切换**：版本页新增「npm 源」下拉与「网络环境」实时显示

### 🔍 更新检查
- **结果可见**：检查更新后会在设置页显示一行「GitHub 最新版本：vX.Y.Z（当前 vX.Y.Z）」，并明确区分「已是最新 / 有更新 / 检查失败」，不再无反馈
- **请求加固**：GitHub 请求加 10 秒超时，失败原因写入日志

### 🐛 修复
- **退出变快**：移除退出时最多 2 秒的进程等待，托盘「退出」瞬时完成
- **消除 cmd 黑窗**：所有子进程调用（node 探测、where、taskkill、npm / pnpm / git / winget）均隐藏控制台窗口，启动 / 退出 / 打开设置不再闪黑窗
- **dsh 更新更可靠**：`npm install -g` 增加 `--force`，可覆盖损坏的 dsh 包
- **dsh 安装探测**（v1.2.1）：直接执行 `dsh web`，仅在找不到全局 dsh 时才走 npm 安装，避免慢启动误判
- **版本探测无副作用**（v1.2.1）：设置页检查 dsh 版本不再触发包下载

### ⚠️ 已知限制
- Mica / Acrylic 依赖 WebView2 对 `backdrop-filter` 的支持，个别老旧设备可能降级为半透明（不影响使用）
- git 源扩展安装仍需手动在 `pnpm-workspace.yaml` 的 `allowBuilds` 放行（添加前有提示）

## 📦 下载

| 文件 | 架构 | 平台 | 类型 | 备注 |
| -- | -- | -- | -- | -- |
| `DeepseekHarness_1.3.0_x64-setup.exe` | x64 | windows | 安装包 | NSIS 安装器（约 3.8 MB） |
| `DeepseekHarness.Setup.exe` | x64 | windows | 安装包 | MewUI 安装器（约 39 MB，已启用单文件压缩） |

> 💡 **国内用户建议直接下载 NSIS 版本**：体积仅约 3.8 MB，而 MewUI 安装器约 39 MB（大 10 倍），墙内下载会明显更慢。
>
> 注：MewUI 安装器本版起启用 .NET 单文件压缩（`EnableCompressionInSingleFile`），
> 体积由 80 MB 降至 39 MB；仍为自包含（用户无需安装 .NET），首次启动需解压、慢约 0.5–1 秒。

**Full Changelog**: https://github.com/AmengBro/Deepseek-Harness-Starter/compare/v1.2.0...v1.3.0

---

## v1.2.1

### 修复
- **dsh 安装探测**：启动方式改为直接执行 `dsh web`；仅在找不到全局 `dsh` 命令时执行 npm 安装，不再根据 `npx` 启动耗时推测安装状态，避免慢启动导致误判和重复安装。
- **版本探测无副作用**：设置页检查 dsh 版本时不再回退到 `npx`，避免一次版本检查意外触发包下载。

---

## v1.2.0

## What's New?

### 新增功能
- **扩展（插件）「包名 / URL 添加」入口**：设置窗口新增「插件」标签，支持按 npm 包名（如 `@scope/name`、`github:owner/repo`）或按 URL（`git+https://...`、tarball `.tar.gz`）添加 dsh 扩展；已安装列表可一键卸载。
- **MCP 管理并入设置窗口**：原独立 MCP 窗口整体移入设置页「MCP」标签，复用更大、可滚动的窗口；新增应用内 Toast 反馈与自定义确认框（替代 Tauri 下不弹窗的原生 `alert`/`confirm`），并补「重启服务生效」按钮以真正加载 MCP / 扩展。
- **pnpm / git 自动安装**：首次添加扩展时若检测到缺 pnpm 或 git 即自动安装（pnpm 经 `npm install -g`，git 经 `winget`），已有则跳过绝不升级；所有 dsh / pnpm 命令加 `--verbose` / `--loglevel=debug`，日志经独立「安装日志」实时回流。

### 修复
- **扩展列表数据源修正（关键）**：原 `dsh plugin list --json` 实际返回 workspace 顶层信息、不含已装插件，改为直接读取 profile 的 `package.json` 的 `dependencies`，列表准确反映用户已装扩展。
- **MCP profile 动态探测**：后端写配置时动态探测当前真实 profile（启动器 `dsh web` → `web`），消除 web / headless 错配。
- **PATH 注入改为子进程级**：移除 `unsafe std::env::set_var` 全局注入（多线程 UB 风险），改为给 dsh 子进程传增强 PATH，更安全地让 dsh 内部找到 pnpm / git。

### 已知限制
- git 源扩展（`github:` / `git+`）安装时 pnpm 可能拦截 `prepare` 构建脚本，需手动在 `pnpm-workspace.yaml` 的 `allowBuilds` 放行，添加前会弹窗提示。
- 设置页其它位置（保存 / 更新 / 检查等）仍在使用原生 `alert`，后续可统一替换为 Toast。

---

## v1.1.0

### 🆕 新增

- **dsh 内核自动更新**：设置窗口新增「dsh 内核」区块，一键检查 / 更新 dsh 到 npm latest（`npm install -g @deepseek-ai/dsh@latest`），安装日志实时滚动。
- **托盘菜单「管理」子菜单**：整合「打开 Skills 文件夹」「设置」「打开安装目录」「打开日志目录」四项入口。
- **强化单实例防双开**：第二个进程不再静默退出，必定把已有窗口置顶到前台。

### 🐛 修复

- **检查更新 API repo 名错误**（关键）：`update.rs` 写成了不存在的 `deepseek-harness`，导致 v1.0.3 的检查更新一直 404、永远检测不到新版本。现已修正为真实仓库 `AmengBro/Deepseek-Harness-Starter`。
- **设置界面版本号硬编码漂移**：现在运行时由 `getVersion()` 从 `tauri.conf.json` 动态读取，与检查更新同源。
- **dsh 版本漂移**：`spawn_dsh_process` 启动命令改为 `@deepseek-ai/dsh@latest`，避免 npx 缓存锁旧版 dsh。
- **Cargo.toml 版本号遗漏**：之前 1.0.3 → 1.1.0 升级时漏改，这次同步到 1.1.0。

### 🔧 重构

- 启动器壳程序版本号（设置界面显示 + 检查更新比较）统一以 `tauri.conf.json` 为唯一真相源。
- C# MewUI 安装器项目版本号同步升至 1.1.0。

### ⚠️ 升级提示

- 由于 v1.0.3 的检查更新 API 地址错误，**v1.0.3 无法自动升级到 v1.1.0**，请从 GitHub Releases 页面手动下载安装包。
- v1.1.0 及之后即可正常接收自动更新。

---

# DeepseekHarness v1.0.3 发行说明

首个开源发布（[GPL-3.0](LICENSE)）。DeepseekHarness 是一个基于 Tauri 的轻量级桌面启动器：自动检测 Node.js、一键启动 DeepSeek Harness 本地 Web 服务，并在内嵌 WebView 中展示服务界面。

## ✨ 功能特性

- 一键启动 `npx @deepseek-ai/dsh web`，服务日志实时显示在主界面
- 智能依赖管理：`npx` 超过 10 秒未就绪时自动切换为 `npm install -g @deepseek-ai/dsh --verbose`，下载全过程实时反馈，避免误以为卡死
- Node.js 自动检测（多策略定位 `node.exe`）
- 内嵌 WebView 展示服务界面（默认端口 3080，占用时自动 +1，仅本次运行生效）
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
- 修复程序可双开的问题（重复启动会自动退出新实例并唤起已运行的主窗口）
- 更换与 DeepSeek 官方近似的 bundle 标识符，避免商标混淆

## 🧰 安装包

- **MewUI 安装器**（推荐）：`DeepseekHarness.Setup-1.0.3-win-x64.exe`（单文件自包含，免管理员）
- **NSIS 安装器**：`DeepseekHarness_1.0.3_x64-setup.exe`（Tauri 官方打包，体积更小）

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
