import { TauriApi, AppConfig, McpServerInput, ExtensionInfo } from "../services/tauri-api";
import { getVersion } from "@tauri-apps/api/app";

const api = new TauriApi();

async function init(): Promise<void> {
    // 动态显示版本号（取自 tauri.conf.json，避免硬编码漂移导致与检查更新对不上）
    const versionText = document.getElementById("version-text") as HTMLSpanElement;
    try {
        versionText.textContent = "v" + (await getVersion());
    } catch {
        versionText.textContent = "未知";
    }

    const portInput = document.getElementById("port-input") as HTMLInputElement;
    const themeSelect = document.getElementById("theme-select") as HTMLSelectElement;
    const btnSave = document.getElementById("btn-save") as HTMLButtonElement;
    const btnCheckUpdate = document.getElementById("btn-check-update") as HTMLButtonElement;
    const btnOpenNodejs = document.getElementById("btn-open-nodejs") as HTMLButtonElement;
    const btnOpenGithub = document.getElementById("btn-open-github") as HTMLButtonElement;
    const btnOpenLogFolder = document.getElementById("btn-open-log-folder") as HTMLButtonElement;
    const btnOpenInstallDir = document.getElementById("btn-open-install-dir") as HTMLButtonElement;
    const btnCheckDsh = document.getElementById("btn-check-dsh") as HTMLButtonElement;
    const btnUpdateDsh = document.getElementById("btn-update-dsh") as HTMLButtonElement;
    const dshCurrent = document.getElementById("dsh-current") as HTMLSpanElement;
    const dshLatest = document.getElementById("dsh-latest") as HTMLSpanElement;
    const dshStatus = document.getElementById("dsh-status") as HTMLSpanElement;
    const dshUpdateLog = document.getElementById("dsh-update-log") as HTMLDivElement;

    let currentConfig: AppConfig = { port: 3080, theme: "system" };

    const readPort = (): number => parseInt(portInput.value, 10);
    const isPortValid = (port: number): boolean => !isNaN(port) && port >= 1 && port <= 65535;

    // 加载当前配置
    try {
        currentConfig = await api.loadConfig();
        portInput.value = String(currentConfig.port);
        themeSelect.value = currentConfig.theme || "system";
        applyTheme(currentConfig.theme || "system");
    } catch {
        // 使用默认值
    }

    // 主题切换实时预览
    themeSelect.addEventListener("change", () => {
        applyTheme(themeSelect.value);
    });

    // 保存配置
    btnSave.addEventListener("click", async () => {
        const port = readPort();
        if (!isPortValid(port)) {
            alert("请输入有效端口号 (1-65535)");
            return;
        }

        const newConfig: AppConfig = { port, theme: themeSelect.value };
        btnSave.disabled = true;
        btnSave.textContent = "保存中...";

        try {
            // 停止当前服务
            try { await api.stopService(); } catch { /* 忽略 */ }

            // 保存配置
            await api.saveConfig(newConfig);

            // 重启服务
            try {
                await api.startService(newConfig);
            } catch { /* 忽略启动错误 */ }
        } catch (err) {
            alert(`保存失败: ${err}`);
        } finally {
            btnSave.disabled = false;
            btnSave.textContent = "保存";
        }
    });

    // 打开日志文件夹（config/applog）
    btnOpenLogFolder.addEventListener("click", async () => {
        try {
            await api.openLogFolder();
        } catch (err) {
            alert(`打开日志文件夹失败: ${err}`);
        }
    });

    // 打开安装目录
    btnOpenInstallDir.addEventListener("click", async () => {
        try {
            await api.openInstallDir();
        } catch (err) {
            alert(`打开安装目录失败: ${err}`);
        }
    });

    // 打开 Skills 文件夹
    const btnOpenSkills = document.getElementById("btn-open-skills") as HTMLButtonElement;
    btnOpenSkills.addEventListener("click", async () => {
        try {
            await api.openSkillsFolder();
        } catch (err) {
            alert(`打开 Skills 文件夹失败: ${err}`);
        }
    });

    // 关闭设置窗口时自动保存未保存的修改
    const { getCurrent } = await import("@tauri-apps/api/window");
    const currentWindow = getCurrent();
    await currentWindow.onCloseRequested(async (event) => {
        const port = readPort();
        if (!isPortValid(port)) {
            event.preventDefault();
            alert("请输入有效端口号 (1-65535)");
            return;
        }
        try {
            await api.saveConfig({ port, theme: themeSelect.value });
        } catch (err) {
            event.preventDefault();
            alert(`自动保存失败: ${err}`);
        }
    });

    // 检查更新（壳程序自身）
    btnCheckUpdate.addEventListener("click", async () => {
        btnCheckUpdate.disabled = true;
        const originalText = btnCheckUpdate.innerHTML;
        btnCheckUpdate.textContent = "检查中...";
        try {
            const result = await api.checkForUpdates();
            if (result) {
                alert(`发现新版本 ${result.version}！\n即将打开下载页面...`);
                await api.openUrl(result.html_url);
            } else {
                alert("当前已是最新版本");
            }
        } catch {
            alert("检查更新失败，请稍后重试");
        } finally {
            btnCheckUpdate.disabled = false;
            btnCheckUpdate.innerHTML = originalText;
        }
    });

    // ---- DeepSeek Harness (dsh) 自动更新 ----
    const refreshDshInfo = async (): Promise<void> => {
        try {
            const info = await api.checkDshVersion();
            dshCurrent.textContent = info.current;
            dshLatest.textContent = info.latest;
            if (info.latest === "unknown") {
                dshStatus.textContent = "检测失败";
            } else if (info.needs_update) {
                dshStatus.textContent = "有更新可用";
            } else {
                dshStatus.textContent = "已是最新";
            }
        } catch {
            dshStatus.textContent = "检测失败";
        }
    };

    // 实时回流 dsh 更新日志
    await api.onDshUpdateLog((log: string) => {
        dshUpdateLog.style.display = "block";
        dshUpdateLog.textContent += log + "\n";
        dshUpdateLog.scrollTop = dshUpdateLog.scrollHeight;
    });

    btnCheckDsh.addEventListener("click", async () => {
        btnCheckDsh.disabled = true;
        const original = btnCheckDsh.textContent;
        btnCheckDsh.textContent = "检查中...";
        try {
            await refreshDshInfo();
        } finally {
            btnCheckDsh.disabled = false;
            btnCheckDsh.textContent = original;
        }
    });

    btnUpdateDsh.addEventListener("click", async () => {
        if (!confirm("确定将 dsh 更新到最新版？更新后建议重启 DeepSeek Harness 服务以生效。")) {
            return;
        }
        btnUpdateDsh.disabled = true;
        btnCheckDsh.disabled = true;
        dshUpdateLog.style.display = "block";
        dshUpdateLog.textContent = "";
        try {
            const newVer = await api.updateDsh();
            alert(`dsh 已更新至 ${newVer}，建议重启 DeepSeek Harness 服务以生效。`);
            await refreshDshInfo();
        } catch (err) {
            alert(`更新失败: ${err}`);
        } finally {
            btnUpdateDsh.disabled = false;
            btnCheckDsh.disabled = false;
        }
    });

    // 进入设置时自动检测一次 dsh 版本
    refreshDshInfo();

    // 打开 Node.js 官网
    btnOpenNodejs.addEventListener("click", async () => {
        await api.openUrl("https://nodejs.org");
    });

    // 初始化并入设置页的 MCP 管理面板
    setupMcpPanel(currentConfig);

    // 初始化并入设置页的扩展（插件）管理面板
    setupExtensionsPanel(currentConfig);

    // 打开 GitHub 项目主页
    btnOpenGithub.addEventListener("click", async () => {
        await api.openUrl("https://github.com/AmengBro/Deepseek-Harness-Starter");
    });
}

// ───────── MCP 管理面板（已并入设置，反馈改用 Toast/自定义确认，不再依赖原生 alert） ─────────
function setupMcpPanel(currentConfig: AppConfig): void {
    // 轻量 Toast 反馈
    const showToast = (msg: string, type: "" | "success" | "error" = ""): void => {
        let t = document.getElementById("mcp-toast");
        if (!t) {
            t = document.createElement("div");
            t.id = "mcp-toast";
            t.className = "toast";
            document.body.appendChild(t);
        }
        const el = t as HTMLElement;
        el.textContent = msg;
        el.className = "toast" + (type ? " " + type : "");
        requestAnimationFrame(() => el.classList.add("show"));
        window.setTimeout(() => el.classList.remove("show"), 3200);
    };

    // 自定义确认框（替代原生 confirm，Tauri 下原生 confirm 不弹窗）
    const showConfirm = (message: string): Promise<boolean> => {
        return new Promise((resolve) => {
            const overlay = document.createElement("div");
            overlay.className = "modal-overlay";
            overlay.innerHTML = `
                <div class="modal-box">
                    <div class="modal-message">${message}</div>
                    <div class="modal-actions">
                        <button class="btn" data-act="cancel">取消</button>
                        <button class="btn btn-primary" data-act="ok">确定</button>
                    </div>
                </div>`;
            document.body.appendChild(overlay);
            overlay.querySelector('[data-act="cancel"]')!.addEventListener("click", () => {
                overlay.remove();
                resolve(false);
            });
            overlay.querySelector('[data-act="ok"]')!.addEventListener("click", () => {
                overlay.remove();
                resolve(true);
            });
        });
    };

    type ServerSummary = { id: string; server_name: string; transport: string; command?: string; url?: string };

    const escapeHtml = (s: string): string => {
        const map: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
        return s.replace(/[&<>"']/g, (c) => map[c] ?? c);
    };
    const inferServerName = (prefix: string): string =>
        prefix.toLowerCase().replace(/[^a-z0-9_-]/g, "-").slice(0, 32) || "server";

    // tab 切换
    document.querySelectorAll<HTMLButtonElement>(".mcp-tab-btn").forEach((btn) => {
        btn.addEventListener("click", () => {
            const target = btn.dataset.tab!;
            document.querySelectorAll(".mcp-tab-btn").forEach((b) => b.classList.toggle("active", b === btn));
            document.querySelectorAll<HTMLElement>(".mcp-tab-panel").forEach((p) =>
                p.classList.toggle("active", p.dataset.panel === target)
            );
        });
    });

    // transport 切换字段显示
    const transportSelect = document.getElementById("mcp-manual-transport") as HTMLSelectElement;
    transportSelect.addEventListener("change", () => {
        const t = transportSelect.value;
        document.querySelectorAll<HTMLElement>("[data-fields]").forEach((el) => {
            const fields = (el.dataset.fields || "").split(",");
            el.style.display = fields.includes(t) ? "" : "none";
        });
    });

    // 打开配置文件
    document.getElementById("mcp-btn-open-config")!.addEventListener("click", async () => {
        try {
            await api.openMcpConfigFile();
        } catch (e) {
            showToast("打开配置文件失败: " + e, "error");
        }
    });

    // 重启服务使 MCP 配置生效（真正“连接”）
    document.getElementById("mcp-btn-restart")!.addEventListener("click", async () => {
        const btn = document.getElementById("mcp-btn-restart") as HTMLButtonElement;
        btn.disabled = true;
        const orig = btn.textContent;
        btn.textContent = "重启中...";
        try {
            try {
                await api.stopService();
            } catch {
                /* 忽略 */
            }
            await new Promise((r) => setTimeout(r, 400));
            try {
                await api.startService(currentConfig);
            } catch {
                /* 忽略启动错误 */
            }
            showToast("已重启服务，MCP 配置已生效", "success");
        } catch (e) {
            showToast("重启失败: " + e, "error");
        } finally {
            btn.disabled = false;
            btn.textContent = orig;
        }
    });

    // 取消按钮
    document.getElementById("mcp-quick-cancel")!.addEventListener("click", () => {
        (document.getElementById("mcp-quick-input") as HTMLTextAreaElement).value = "";
    });
    document.getElementById("mcp-manual-cancel")!.addEventListener("click", () => clearManual());
    document.getElementById("mcp-json-cancel")!.addEventListener("click", () => {
        (document.getElementById("mcp-json-input") as HTMLTextAreaElement).value = "";
    });

    // 添加按钮
    document.getElementById("mcp-quick-add")!.addEventListener("click", () => quickAdd());
    document.getElementById("mcp-manual-add")!.addEventListener("click", () => manualAdd());
    document.getElementById("mcp-json-add")!.addEventListener("click", () => jsonAdd());

    async function refreshAll(): Promise<void> {
        try {
            const cfg = await api.getMcpConfigPath();
            document.getElementById("mcp-config-path")!.textContent = `路径: ${cfg.path}`;
        } catch (e) {
            document.getElementById("mcp-config-path")!.textContent = `路径错误: ${e}`;
        }
        try {
            const servers = await api.listMcpServers();
            renderServerList(servers);
        } catch (e) {
            renderServerList([]);
            showToast("读取 MCP 列表失败: " + e, "error");
        }
    }

    function renderServerList(servers: ServerSummary[]): void {
        const list = document.getElementById("mcp-server-list")!;
        if (servers.length === 0) {
            list.innerHTML = `<div class="mcp-empty">暂无 MCP server，使用下方任一方式添加</div>`;
            return;
        }
        list.innerHTML = servers
            .map((s) => {
                const detail = s.transport === "stdio" ? s.command ?? "-" : s.url ?? "-";
                return `<div class="mcp-server-item" data-id="${escapeHtml(s.id)}">
                    <div>
                        <span class="mcp-server-name">${escapeHtml(s.server_name)}</span>
                        <span class="mcp-server-meta">[${escapeHtml(s.transport)}] ${escapeHtml(detail)}</span>
                    </div>
                    <button class="btn mcp-btn-danger" data-remove="${escapeHtml(s.id)}">删除</button>
                </div>`;
            })
            .join("");
        list.querySelectorAll<HTMLButtonElement>("[data-remove]").forEach((btn) => {
            btn.addEventListener("click", async () => {
                const id = btn.dataset.remove!;
                if (!(await showConfirm(`删除 MCP server '${id}'？`))) return;
                try {
                    await api.removeMcpServer(id);
                    await refreshAll();
                    showToast("已删除 " + id, "success");
                } catch (e) {
                    showToast("删除失败: " + e, "error");
                }
            });
        });
    }

    function clearManual(): void {
        (document.getElementById("mcp-manual-name") as HTMLInputElement).value = "";
        (document.getElementById("mcp-manual-command") as HTMLInputElement).value = "";
        (document.getElementById("mcp-manual-args") as HTMLInputElement).value = "";
        (document.getElementById("mcp-manual-env") as HTMLTextAreaElement).value = "";
        (document.getElementById("mcp-manual-url") as HTMLInputElement).value = "";
        (document.getElementById("mcp-manual-headers") as HTMLTextAreaElement).value = "";
    }

    function quickAdd(): void {
        const raw = (document.getElementById("mcp-quick-input") as HTMLTextAreaElement).value.trim();
        if (!raw) {
            showToast("请输入命令、URL 或 JSON", "error");
            return;
        }
        let input: McpServerInput;
        if (raw.startsWith("{")) {
            try {
                input = JSON.parse(raw);
            } catch (e) {
                showToast("JSON 解析失败: " + e, "error");
                return;
            }
        } else if (/^https?:\/\//i.test(raw)) {
            input = {
                server_name: inferServerName(new URL(raw).hostname),
                transport: "streamable-http",
                url: raw,
            };
        } else {
            const parts = raw.split(/\s+/).filter(Boolean);
            if (parts.length === 0) {
                showToast("命令无效", "error");
                return;
            }
            input = {
                server_name: inferServerName(parts[0]),
                transport: "stdio",
                command: parts[0],
                args: parts.slice(1),
            };
        }
        if (!input.server_name) {
            showToast("缺少 serverName", "error");
            return;
        }
        if (!input.transport) {
            showToast("缺少 transport", "error");
            return;
        }
        submitAdd(input, "mcp-quick-input");
    }

    function manualAdd(): void {
        const name = (document.getElementById("mcp-manual-name") as HTMLInputElement).value.trim();
        const transport = (document.getElementById("mcp-manual-transport") as HTMLSelectElement).value;
        if (!name) {
            showToast("请填写名称", "error");
            return;
        }
        const input: McpServerInput = { server_name: name, transport };
        if (transport === "stdio") {
            input.command = (document.getElementById("mcp-manual-command") as HTMLInputElement).value.trim();
            if (!input.command) {
                showToast("stdio 模式必须填写命令", "error");
                return;
            }
            const argsRaw = (document.getElementById("mcp-manual-args") as HTMLInputElement).value.trim();
            if (argsRaw) input.args = argsRaw.split(/\s+/).filter(Boolean);
            const envRaw = (document.getElementById("mcp-manual-env") as HTMLTextAreaElement).value.trim();
            if (envRaw) {
                input.env = {};
                for (const line of envRaw.split(/\r?\n/)) {
                    const m = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/);
                    if (m) input.env[m[1]] = m[2];
                }
            }
        } else {
            input.url = (document.getElementById("mcp-manual-url") as HTMLInputElement).value.trim();
            if (!input.url) {
                showToast("streamable-http 模式必须填写 URL", "error");
                return;
            }
            const hdrRaw = (document.getElementById("mcp-manual-headers") as HTMLTextAreaElement).value.trim();
            if (hdrRaw) {
                input.headers = {};
                for (const line of hdrRaw.split(/\r?\n/)) {
                    const m = line.match(/^\s*([A-Za-z0-9_-]+)\s*:\s*(.*?)\s*$/);
                    if (m) input.headers[m[1]] = m[2];
                }
            }
        }
        submitAdd(input, null);
        clearManual();
    }

    function jsonAdd(): void {
        const raw = (document.getElementById("mcp-json-input") as HTMLTextAreaElement).value.trim();
        if (!raw) {
            showToast("请输入 JSON", "error");
            return;
        }
        let obj: any;
        try {
            obj = JSON.parse(raw);
        } catch (e) {
            showToast("JSON 解析失败: " + e, "error");
            return;
        }
        submitAdd(obj as McpServerInput, "mcp-json-input");
    }

    async function submitAdd(input: McpServerInput, clearFieldId: string | null): Promise<void> {
        try {
            const id = await api.addMcpServer(input);
            showToast(`已添加 '${id}'，重启服务后生效`, "success");
            await refreshAll();
            if (clearFieldId) {
                (document.getElementById(clearFieldId) as HTMLInputElement | HTMLTextAreaElement).value = "";
            }
        } catch (e) {
            showToast("添加失败: " + e, "error");
        }
    }

    // 初次加载
    refreshAll();
}

// ───────── 扩展（插件）管理面板（已并入设置，反馈用 Toast/确认框） ─────────
function setupExtensionsPanel(currentConfig: AppConfig): void {
    const showToast = (msg: string, type: "" | "success" | "error" = ""): void => {
        let t = document.getElementById("ext-toast");
        if (!t) {
            t = document.createElement("div");
            t.id = "ext-toast";
            t.className = "toast";
            document.body.appendChild(t);
        }
        const el = t as HTMLElement;
        el.textContent = msg;
        el.className = "toast" + (type ? " " + type : "");
        requestAnimationFrame(() => el.classList.add("show"));
        window.setTimeout(() => el.classList.remove("show"), 3200);
    };

    const showConfirm = (message: string): Promise<boolean> => {
        return new Promise((resolve) => {
            const overlay = document.createElement("div");
            overlay.className = "modal-overlay";
            overlay.innerHTML = `
                <div class="modal-box">
                    <div class="modal-message">${message}</div>
                    <div class="modal-actions">
                        <button class="btn" data-act="cancel">取消</button>
                        <button class="btn btn-primary" data-act="ok">确定</button>
                    </div>
                </div>`;
            document.body.appendChild(overlay);
            overlay.querySelector('[data-act="cancel"]')!.addEventListener("click", () => {
                overlay.remove();
                resolve(false);
            });
            overlay.querySelector('[data-act="ok"]')!.addEventListener("click", () => {
                overlay.remove();
                resolve(true);
            });
        });
    };

    const escapeHtml = (s: string): string => {
        const map: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
        return s.replace(/[&<>"']/g, (c) => map[c] ?? c);
    };

    const pkgInput = document.getElementById("ext-pkg-input") as HTMLInputElement;
    const urlInput = document.getElementById("ext-url-input") as HTMLInputElement;
    const listEl = document.getElementById("ext-list") as HTMLDivElement;
    const logEl = document.getElementById("ext-log") as HTMLDivElement;
    const depsEl = document.getElementById("ext-deps") as HTMLDivElement;

    const appendLog = (msg: string): void => {
        logEl.style.display = "block";
        logEl.textContent += msg + "\n";
        logEl.scrollTop = logEl.scrollHeight;
    };

    // 实时回流扩展安装/列出日志
    api.onExtensionLog((log: string) => appendLog(log));

    // 重启服务使扩展生效
    document.getElementById("ext-btn-restart")!.addEventListener("click", async () => {
        const btn = document.getElementById("ext-btn-restart") as HTMLButtonElement;
        btn.disabled = true;
        const orig = btn.textContent;
        btn.textContent = "重启中...";
        try {
            try {
                await api.stopService();
            } catch {
                /* 忽略 */
            }
            await new Promise((r) => setTimeout(r, 400));
            try {
                await api.startService(currentConfig);
            } catch {
                /* 忽略启动错误 */
            }
            showToast("已重启服务，扩展已生效", "success");
        } catch (e) {
            showToast("重启失败: " + e, "error");
        } finally {
            btn.disabled = false;
            btn.textContent = orig;
        }
    });

    // 手动检查并安装依赖
    document.getElementById("ext-btn-checkdeps")!.addEventListener("click", async () => {
        appendLog("[系统] 正在检查并安装依赖（pnpm / git）...");
        try {
            await api.ensureDeps();
            showToast("依赖就绪（pnpm / git）", "success");
        } catch (e) {
            showToast("依赖检查失败: " + e, "error");
        }
    });

    async function refreshDeps(): Promise<void> {
        depsEl.textContent =
            "依赖（pnpm / git）将在首次「添加」时自动检测并安装；如安装失败，可点「检查并安装依赖」。";
    }

    async function refreshList(): Promise<void> {
        try {
            const exts = await api.listExtensions();
            renderList(exts);
        } catch (e) {
            renderList([]);
            showToast("读取扩展列表失败: " + e, "error");
        }
    }

    function renderList(exts: ExtensionInfo[]): void {
        if (exts.length === 0) {
            listEl.innerHTML = `<div class="mcp-empty">暂无已安装扩展，使用下方「按包名」或「按 URL」添加</div>`;
            return;
        }
        listEl.innerHTML = exts
            .map((e) => {
                return `<div class="mcp-server-item" data-name="${escapeHtml(e.name)}">
                    <div>
                        <span class="mcp-server-name">${escapeHtml(e.name)}</span>
                        <span class="mcp-server-meta">${escapeHtml(e.version)}</span>
                    </div>
                    <button class="btn mcp-btn-danger" data-remove="${escapeHtml(e.name)}">卸载</button>
                </div>`;
            })
            .join("");
        listEl.querySelectorAll<HTMLButtonElement>("[data-remove]").forEach((btn) => {
            btn.addEventListener("click", async () => {
                const name = btn.dataset.remove!;
                if (!(await showConfirm(`卸载扩展 '${name}'？`))) return;
                try {
                    await api.uninstallExtension(name);
                    await refreshList();
                    showToast("已卸载 " + name, "success");
                } catch (e) {
                    showToast("卸载失败: " + e, "error");
                }
            });
        });
    }

    function isGitSource(s: string): boolean {
        return s.startsWith("github:") || s.startsWith("git+") || s.endsWith(".git");
    }

    async function addBySource(raw: string, clearInput: HTMLInputElement): Promise<void> {
        const source = raw.trim();
        if (!source) {
            showToast("请输入来源", "error");
            return;
        }
        if (isGitSource(source)) {
            const ws = "%USERPROFILE%/.dsh/profiles/web/pnpm-workspace.yaml";
            const ok = await showConfirm(
                `检测到 git 源，pnpm 安装时会拦截 prepare 构建脚本。\n若安装卡住，请手动在 ${ws} 的 allowBuilds 中加入：\n  - ${source}\n\n确定仍要继续安装吗？`
            );
            if (!ok) return;
        }
        try {
            await api.installExtension(source);
            showToast(`已添加 '${source}'，重启服务后生效`, "success");
            await refreshList();
            clearInput.value = "";
        } catch (e) {
            showToast("添加失败: " + e, "error");
        }
    }

    document
        .getElementById("ext-pkg-add")!
        .addEventListener("click", () => addBySource(pkgInput.value, pkgInput));
    document.getElementById("ext-pkg-cancel")!.addEventListener("click", () => {
        pkgInput.value = "";
    });
    document
        .getElementById("ext-url-add")!
        .addEventListener("click", () => addBySource(urlInput.value, urlInput));
    document.getElementById("ext-url-cancel")!.addEventListener("click", () => {
        urlInput.value = "";
    });

    refreshDeps();
    refreshList();
}

function applyTheme(theme: string): void {
    let resolved = theme;
    if (theme === "system") {
        resolved = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    }
    document.documentElement.setAttribute("data-theme", resolved);
}

document.addEventListener("DOMContentLoaded", () => {
    init();
});
