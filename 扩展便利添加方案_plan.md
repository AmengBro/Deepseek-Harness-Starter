# DeepseekHarness 扩展便利添加方案（Skills / MCP / 插件）— v1.1.0

> 状态：**规划（Plan）**，尚未实现。本文件为设计草案，供评审与排期。
> 关联版本：`package.json` / `src-tauri/tauri.conf.json` 已升至 `1.1.0`。

---

## 1. 背景与目标

DeepSeek Harness（命令行 `dsh`）是 2026-08-13 开源的 Agent 运行框架，核心设计理念是 **「一切皆插件」**（基于 Cordis 插件内核）：模型、工具、Skills、会话、沙箱、存储、UI 全部以插件形式存在，可自由替换、卸载、扩展。

但「可扩展」不等于「易扩展」。当前普通用户要往 dsh 里加能力，仍要面对：
- **Skills**：手动把含 `SKILL.md` 的文件夹丢进 `~/.agents/skills/`，或敲 `npx skills`；
- **插件**：敲 `dsh plugin --profile <配置名> add <包名>`；
- **MCP**：配置繁琐，社区甚至专门写了 `dsh-mcp-install` 这类 Skill 来「让 AI 帮配 MCP」。

**目标**：在 DeepseekHarness 启动器（Tauri 桌面应用）内，提供 **可视化、一键式** 的扩展管理面板，让普通用户无需碰命令行即可浏览、安装、卸载 Skills / MCP / 插件，降低 dsh 生态的使用门槛。

---

## 2. 现状分析（基于现有代码）

启动器当前架构（可直接复用）：

| 层 | 位置 | 可复用点 |
|----|------|----------|
| 后端命令注册 | `src-tauri/src/lib.rs` 的 `generate_handler!` | 新增 `#[tauri::command]` 在此登记即可被前端调用 |
| 命令实现 | `src-tauri/src/commands/*.rs` | 已有 `config`、`service`、`update` 三个模块；新建 `extensions` 模块 |
| 命令执行范式 | `commands/service.rs` 的 `spawn_dsh_process` / `run_npm_install` | 已封装 `cmd /c` + 子进程 + 管道读取，可直接拿来做 `dsh plugin add` 等 |
| 日志回流 | `emit_all("service-log", line)` + 前端 `listen("service-log")` | 安装进度可复用同一事件流（或新增 `extension-log`） |
| 窗口创建 | `lib.rs` 的 `open_settings_inner`（用 `WindowBuilder` 开 `settings.html`） | 「扩展管理」窗口可照搬此范式 |

前端范式：`src/services/tauri-api.ts` 封装 `invoke()` 调用；`src/settings/main.ts` + `settings.html` 是独立窗口的范例。

### dsh 扩展体系现状（需落地前核实）

| 扩展类型 | 磁盘位置 / 命令 | 备注 |
|----------|----------------|------|
| **Skills** | `~/.agents/skills/<name>/SKILL.md`；安装可用 `npx skills`；调用用 `/<skill名>` | 路径随 OS 变化（Windows 为 `%USERPROFILE%\.agents\skills`），**需核实** |
| **插件** | `dsh plugin --profile <profile> add <pkg>` / `remove <pkg>`（例：`dsh plugin --profile tui add @openma/deepseek-harness-tui`） | 需确认默认 profile 名、`dsh` 全局可用时的 PATH 解析（启动器已是 GUI，PATH 可能不全，复用 `service.rs` 的 `cmd /c` 思路） |
| **MCP** | 配置文件路径与格式 **尚未确认**（可能是 `settings.yaml` 内片段或独立 `.mcp.json`；profile 如何作用其上需核实） | 社区 `dsh-mcp-install` Skill 即因配置繁琐而存在 → 这是「便利添加」价值最大的点 |

### dsh 自身版本更新机制（实测 · 2026-08-19）

> 启动器不负责更新 dsh，但理解 dsh 怎么更新，才能判断「便利添加」功能是否会被版本漂移坑到。

- **npm 最新版（`latest` 标签）**：`0.1.0-rc.7`（dist-tags: `latest` = `next` = `rc.7`）。
- **全局兜底安装**（`service.rs` 的 `npm install -g @deepseek-ai/dsh`）：本机实测为 `0.1.0-rc.6`，落后一个版本，**且永远不会自动更新**——除非手动 `npm update -g`。
- **npx 默认路径**（`npx --yes @deepseek-ai/dsh`）：本机实测输出 `0.1.0-rc.6`。看似「自动更新」，实则 npx 有本地缓存（`~/.npm/_npx`），一旦缓存过 rc.6 就会一直复用，**不会主动拉 rc.7**。
- **显式 `npx @deepseek-ai/dsh@latest`**：实测还会撞上 npx 缓存目录的并发锁错误（concurrency.lock trash 失败），进一步说明这套机制并不稳健。

**结论**：dsh「npx 自动更新」在设计与口号上是成立的，但**在实践中不可靠**——本机两条启动路径（npx 缓存 / 全局兜底）都停在 rc.6，并未追上 rc.7。光看启动器日志（`npm install -g ...` vs 直接 npx）只能区分路径，**看不出版本号**，而两条路径版本恰好一致，因此「投机取巧看日志」无法发现这种版本漂移。

---

## 3. 设计原则

1. **复用而非重写**：直接使用启动器已有的「命令下发 + 日志回流 + 独立窗口」三件套，不引入新框架。
2. **直接操作磁盘与 CLI**：绕过 dsh Web UI 内部实现，直接写 Skills 目录、调用 `dsh plugin`、改写 MCP 配置——但所有路径/命令都做 **版本探测 + 失败回退 + 友好提示**，应对 dsh dev preview 的破坏性变更。
3. **清单与 dsh 解耦**：把「启动器托管 / 已安装」的扩展清单存到启动器自己的 `config/extensions.json`，只记录「类型、标识、来源、安装时间」，不替 dsh 管理其完整配置，避免越权与冲突。
4. **安全第一**：仅操作已知目录（`~/.agents/skills`、dsh 配置目录），安装前校验来源（白名单 / URL 合法性）并弹确认；绝不触碰系统关键文件或注册表。

---

## 4. 方案总览（三层）

```
┌─────────────────────────────────────────────┐
│  前端：扩展管理窗口 (extensions.html)          │
│  ├─ 标签页：Skills | MCP | 插件               │
│  ├─ 内置目录卡片 + 自定义添加表单              │
│  └─ 状态区 + 日志区（复用 service-log 事件）   │
└───────────────┬─────────────────────────────┘
                │ invoke()
┌───────────────▼─────────────────────────────┐
│  后端：commands/extensions.rs                 │
│  install_skill / uninstall_skill / list_skills│
│  install_plugin / uninstall_plugin / list_... │
│  add_mcp_server / remove_mcp_server / list_.. │
│  （复用 emit_all("service-log") 回流进度）     │
└───────────────┬─────────────────────────────┘
                │ 文件写入 / 子进程 (dsh, npx, git)
┌───────────────▼─────────────────────────────┐
│  dsh 运行时环境                                │
│  ~/.agents/skills/*  ·  dsh plugin CLI  ·     │
│  dsh MCP 配置文件                              │
└─────────────────────────────────────────────┘
```

外加一份随包发布的 **`catalog.json`**（精选扩展目录），以及持久化文件 **`config/extensions.json`**。

---

## 5. 后端命令设计（`src-tauri/src/commands/extensions.rs`）

所有命令复用 `service.rs` 的 `get_install_dir`、`cmd /c` 执行与管道读取范式；进度统一通过 `emit_all("service-log", ...)`（或独立的 `extension-log`）回流。

### 5.1 Skills
- `install_skill(source, name)`
  - `source` 支持三种：`npm:<pkg>`（执行 `npx skills ...` 或下载 SKILL.md）、`git:<url>`（`git clone` 到 skills 目录）、`url:<raw_url>`（直接下载 `SKILL.md`）。
  - 目标目录：`~/.agents/skills/<name>/SKILL.md`（Windows 解析为 `%USERPROFILE%\.agents\skills`）。
  - 校验目标名合法、目录不存在或用户确认覆盖。
- `uninstall_skill(name)`：删除对应目录（移动到回收站式备份更稳妥，先做普通删除 + 二次确认）。
- `list_skills()`：扫描 `~/.agents/skills/` 返回已安装清单（供前端展示）。

### 5.2 插件
- `install_plugin(profile, package)`：执行 `cmd /c dsh plugin --profile <profile> add <package>`，实时回流日志；成功/失败回传状态。
- `uninstall_plugin(profile, package)`：`dsh plugin --profile <profile> remove <package>`。
- `list_plugins(profile)`：`dsh plugin --profile <profile> list`（若支持），否则解析本地 profile 目录。
- 前置：复用 `check_node` 思路确认 `dsh` 可执行；若非全局安装，回退到 `npx @deepseek-ai/dsh ...`。

### 5.3 MCP
- `add_mcp_server(entry)`：`entry` 形如 MCP 标准 `mcpServers.<name>` 片段（`command` / `args` / `env`）。读取 dsh 的 MCP 配置文件（**路径待核实**），按 schema 合并写入；冲突同名提示覆盖。
- `remove_mcp_server(name)`：从配置中移除对应条目并重写文件。
- `list_mcp_servers()`：解析配置文件返回当前条目。
- ⚠️ 关键 Spike：先搞清楚 dsh 在哪、以什么格式存 MCP Server（JSON？YAML？profile 维度？），再定读写逻辑。

### 5.4 持久化
- 安装/卸载成功后，更新 `config/extensions.json`（类型、标识、来源、时间），用于前端列表与卸载校验。

---

## 6. 前端 UI 设计（`extensions.html` + `src/extensions/main.ts`）

- **窗口**：照搬 `open_settings_inner` 范式，新增 `open_extensions_window` 命令与 `extensions.html`；尺寸约 `720 × 560`。
- **布局**：顶部三个标签页 `Skills | MCP | 插件`；每页含
  - **精选目录区**：从 `catalog.json` 渲染卡片（名称、描述、来源、类型徽标、安装/卸载按钮、状态）。
  - **自定义添加区**：表单（粘贴 npm/git/raw URL，或 MCP 的 JSON 片段）→ 调对应后端命令。
  - **状态 + 日志区**：复用 `service-log` / 新增 `extension-log` 事件，实时显示安装进度。
- **入口**：
  - 主界面（`src/app/App.ts`）新增「扩展管理」按钮 → `open_extensions_window`。
  - 托盘菜单（`lib.rs`）可选加「扩展管理」项。
- **交互细节**：安装前弹确认（显示来源 URL）；进行中禁用按钮并显示 spinner；完成后 toast 提示，并提示「插件/MCP 可能需要重启 dsh 服务生效」→ 提供「重启服务」入口（复用 `stop_harness_service` + `start_harness_service`）。

---

## 7. 数据文件

- **`catalog.json`**（随 `dist/` 打包，置于 `resources/` 或 `public/`）：内置若干经核实的扩展，字段：
  ```json
  {
    "skills": [
      { "name": "dsh-mcp-install", "desc": "辅助 AI 配置 MCP Server", "source": "git:https://github.com/LimeBlogs/dsh-mcp-install" }
    ],
    "plugins": [
      { "name": "@openma/deepseek-harness-tui", "desc": "把 dsh 变成终端界面", "profile": "tui", "source": "npm:@openma/deepseek-harness-tui" }
    ],
    "mcp": []
  }
  ```
  > 目录内容需在落地时逐一核实可用性与最新地址，避免硬编码失效链接。
- **`config/extensions.json`**（运行时生成）：启动器托管的扩展清单，仅用于展示与卸载。

---

## 8. 安全与边界

- 仅操作 `~/.agents/skills` 与 dsh 配置目录；来源 URL 做合法性 / 白名单校验。
- 删除类操作必须二次确认；优先「备份后删除」而非直接 rm。
- 安装/卸载走子进程并超时控制，异常捕获后友好提示，不崩溃主程序。
- 插件 / MCP 生效常需重启 dsh 服务 → 明确提示并提供重启入口。
- dsh 处于 dev preview，命令与配置随时可能破坏性变更：所有外部调用前做 **能力探测**（如 `dsh --help` / 版本检查），失败时回退并提示用户查看官方文档。

---

## 9. 实施步骤（里程碑）

| 阶段 | 内容 | 依赖 |
|------|------|------|
| **M1** | 后端 `extensions` 模块的 Skills 安装/卸载/列表 + 前端扩展窗口骨架 | 无（最易验证） |
| **M2** | `catalog.json` + 前端三个标签页与卡片 UI + 日志回流接入 | M1 |
| **M3** | 插件安装/卸载（复用 M1 命令执行框架 + `dsh plugin` CLI） | M1 |
| **M4** | MCP 配置读写（**先做 Spike 核实 dsh MCP 配置格式**） | M1，需先完成 Spike |
| **M5** | 持久化 `extensions.json`、托盘入口、重启服务提示、版本发布（GitHub Release + 安装器） | M2–M4 |

---

## 10. 待核实 / 风险清单（Spike 项）

1. dsh 在 **Windows** 下 Skills 目录的确切路径（`%USERPROFILE%\.agents\skills` 是否准确？）。
2. dsh **MCP Server 配置**的准确文件路径与格式（JSON？YAML？是否按 profile 分文件？）。
3. `dsh plugin` 子命令在全局安装后的可用性、**默认 profile 名**、是否需要 `--profile`。
4. dsh 仍在 dev preview，命令/路径可能破坏性变更 → 必须加版本探测与容错。
5. 启动器是 GUI 进程，PATH 可能不完整（已有 `cmd /c` 范式可解决 node/npx 解析）。
6. **dsh 版本漂移（已实测）**：本机 npx 缓存与全局兜底均停留在 `rc.6`，未自动追上 npm 最新的 `rc.7`（详见第 2 节实测）。「便利添加」功能不能假设「npx 永远最新」，必须显式探测 `dsh --version` 并对比 npm `latest`，在版本落后或过旧时提示用户主动更新；同时建议在 `service.rs` 把全局兜底改为「始终走 npx 最新或显式 `@latest`」，消除一个不自动更新的死版本来源。

---

## 11. 验收标准

- 在扩展管理窗口中，可从内置目录一键安装至少 1 个 Skill、1 个插件；安装进度可见、结果有提示。
- 可一键卸载上述扩展，且 dsh 侧确实不再加载。
- 可图形化添加一个 MCP Server 并持久化；移除后配置正确回写。
- 所有操作不触碰系统关键文件，失败有明确提示，主程序不崩溃。
- 版本号、RELEASE_NOTES 与功能状态一致。
