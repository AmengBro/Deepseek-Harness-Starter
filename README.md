# DeepseekHarnessStarter

DeepSeek Harness 一键启动器 —— 基于 Tauri 的轻量级跨平台桌面工具。自动检测 Node.js、一键启动 `@deepseek-ai/dsh` 本地 Web 服务，并在内嵌 WebView 中展示服务界面。

> 本项目与 DeepSeek / 深度求索无任何隶属关系，DeepSeek 商标归其各自所有者所有。

## 功能特性

- 一键启动 `npx @deepseek-ai/dsh web`，stdout/stderr 实时显示在主界面日志区
- 智能依赖管理：`npx` 超过 10 秒未就绪时，自动切换为 `npm install -g @deepseek-ai/dsh --verbose` 全局安装，下载全过程（npm 详细日志）实时反馈到日志区，避免用户误以为程序卡死
- Node.js 自动检测（多策略定位 `node.exe`）
- 内嵌 WebView 展示服务界面（默认端口 3080）
- 系统托盘常驻：右键菜单提供「打开主界面 / 设置 / 退出」
- 设置窗口：服务端口、打开日志文件夹、打开安装目录，关闭时自动保存
- 日志按日期滚动（`applog` / `npmlog`），保留最近 30 天
- 端口被占用时自动 +1 切换并更新配置
- 服务崩溃自动重启（最多 3 次，间隔 5 秒）
- 基于 GitHub Releases 的自动更新检查

## 环境要求

- 运行时：Node.js 18+（Windows / macOS / Linux）
- 开发构建：Rust toolchain、Node.js 18+、Tauri CLI

## 开发调试

```bash
npm install
npm run tauri dev
```

## 构建发布

```bash
npm run tauri build
```

产物位于 `src-tauri/target/release/bundle/`。

## 安装器（MewUI）

`setup/DeepseekHarness.Setup` 是一个基于 [MewUI](https://github.com/aprillz/MewUI)（Direct2D + Mica 材质，样式参考 Starward.Setup）的按用户安装器，无需管理员权限：

```bash
dotnet publish setup/DeepseekHarness.Setup/DeepseekHarness.Setup.csproj -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:MewUIBackend=Direct2D
```

发布产物为单文件 `DeepseekHarness.Setup.exe`，内嵌应用负载，支持 `/S` 静默安装与 `uninstall /S` 静默卸载。最终安装包可复制到 `dist/` 随 GitHub Release 发布。

## 使用说明

1. 首次启动时工具会自动检测 Node.js；若未安装，会提示前往 [nodejs.org](https://nodejs.org) 下载。
2. 点击「启动服务」，工具自动执行 `npx` 启动 dsh；若判定需要下载依赖，会自动切换到 npm 全局安装并实时显示进度。
3. 服务就绪后自动打开内嵌界面。
4. 点击主窗口关闭按钮会隐藏到系统托盘，右键托盘图标可打开主界面、设置或退出。

## 配置与日志

程序运行数据保存在**应用安装目录**下的 `config/` 文件夹，不依赖注册表或 AppData：

```text
config/
├── config.toml    # 端口等配置（TOML 格式）
├── applog/        # 应用日志，app-YYYY-MM-DD.log，保留 30 天
└── npmlog/        # npm 子进程原始输出，npm-YYYY-MM-DD.log
```

产品需求文档见 [需求文档(goal).md](需求文档(goal).md)。

## 许可证

本项目以 [GPL-3.0](LICENSE) 发布。

第三方组件许可证详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 贡献者

- **[AmengBro](https://github.com/AmengBro)** — 作者 / 维护者
- **Codex (OpenAI)** — 协作开发
