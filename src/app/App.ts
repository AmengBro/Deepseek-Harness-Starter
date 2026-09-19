import { TauriApi, AppConfig, ReleaseInfo, UpdateCheckResult } from "../services/tauri-api";

interface LogLine {
    text: string;
    level: "info" | "error" | "success" | "warning" | "system";
    ts: string;
}

export class App {
    private api: TauriApi;
    private config: AppConfig = { port: 3080, theme: "system" };
    private logs: LogLine[] = [];
    private maxLogs = 200;

    private els = {
        btnMinimize: {} as HTMLButtonElement,
        btnMaximize: {} as HTMLButtonElement,
        btnClose: {} as HTMLButtonElement,
        loadingView: {} as HTMLElement,
        webviewView: {} as HTMLElement,
        terminalBody: {} as HTMLElement,
    };

    constructor() {
        this.api = new TauriApi();
    }

    async init(): Promise<void> {
        this.cacheElements();
        this.bindEvents();
        this.applyTheme("system");
        this.setupEventListeners();
        await this.loadConfig();
        // 后台执行：国内网络下该检测最多等 3 秒，不能阻塞服务启动
        void this.ensureNpmMirror();
        await this.autoStart();
    }

    private cacheElements(): void {
        this.els.btnMinimize = document.getElementById("btn-minimize") as HTMLButtonElement;
        this.els.btnMaximize = document.getElementById("btn-maximize") as HTMLButtonElement;
        this.els.btnClose = document.getElementById("btn-close") as HTMLButtonElement;
        this.els.loadingView = document.getElementById("loading-view") as HTMLElement;
        this.els.webviewView = document.getElementById("webview-view") as HTMLElement;
        this.els.terminalBody = document.getElementById("terminal-body") as HTMLElement;
    }

    private bindEvents(): void {
        // 已改用系统原生标题栏：index.html 里自绘的最小化/最大化/关闭按钮已移除。
        // 这里保留逻辑并做判空保护——元素不存在时静默跳过，日后若要切回自绘标题栏无需改代码。
        this.els.btnMinimize?.addEventListener("click", () => this.api.minimizeWindow());
        this.els.btnMaximize?.addEventListener("click", () => this.api.maximizeWindow());
        this.els.btnClose?.addEventListener("click", () => this.api.closeWindow());
    }

    private setupEventListeners(): void {
        this.api.onServiceLog((log) => this.handleLog(log));
        this.api.onServiceReady((url) => this.handleReady(url));
        this.api.onUpdateAvailable((info) => this.showUpdateDialog(info));
        this.api.onUpdateChecked((r) => this.handleUpdateStatus(r));
    }

    private applyTheme(theme: string): void {
        let resolved = theme;
        if (theme === "system") {
            resolved = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
        }
        document.documentElement.setAttribute("data-theme", resolved);
    }

    private async loadConfig(): Promise<void> {
        try {
            this.config = await this.api.loadConfig();
            this.applyTheme(this.config.theme || "system");
        } catch {
            // 使用默认配置
        }
    }

    /// 启动服务前先保障 npm 源可用：官方源 + 国内网络 → 自动切国内镜像。
    /// 结果日志由 Rust 端通过 service-log 事件回传（切换/失败时才发），此处不重复打印，
    /// 仅兜住异常，绝不因镜像检测失败阻断主流程。
    private async ensureNpmMirror(): Promise<void> {
        try {
            await this.api.ensureNpmMirror();
        } catch (err) {
            this.handleLog(`[警告] npm 源检测失败（不影响启动）: ${err}`);
        }
    }

    private async autoStart(): Promise<void> {
        // 检查 Node.js
        try {
            const version = await this.api.checkNode();
            this.handleLog(`[系统] Node.js 已检测到: ${version}`);
        } catch (err) {
            this.handleLog(`[错误] Node.js 检测失败: ${err}，请前往 nodejs.org 安装`);
            this.showNodeMissingDialog();
            return;
        }

        // 自动启动服务
        try {
            this.handleLog("[系统] 正在启动服务...");
            await this.api.startService(this.config);
        } catch (err) {
            this.handleLog(`[错误] 启动失败: ${err}`);
        }

        // 5秒后检查更新
        setTimeout(() => this.checkUpdates(), 5000);
    }

    private handleLog(log: string): void {
        let level: LogLine["level"] = "info";
        if (log.includes("[错误]") || log.includes("error") || log.includes("Error")) {
            level = "error";
        } else if (log.includes("就绪") || log.includes("ready") || log.includes("listening")) {
            level = "success";
        } else if (log.includes("[系统]")) {
            level = "system";
        } else if (log.includes("警告") || log.includes("warn")) {
            level = "warning";
        }

        const ts = new Date().toLocaleTimeString("zh-CN", { hour12: false });
        this.logs.push({ text: log, level, ts });

        if (this.logs.length > this.maxLogs) {
            this.logs = this.logs.slice(-this.maxLogs);
        }

        this.renderTerminal();
    }

    private renderTerminal(): void {
        const frag = document.createDocumentFragment();
        for (const line of this.logs) {
            const div = document.createElement("div");
            div.className = `terminal-line ${line.level}`;
            const tsSpan = document.createElement("span");
            tsSpan.className = "ts";
            tsSpan.textContent = line.ts;
            div.appendChild(tsSpan);
            div.appendChild(document.createTextNode(line.text));
            frag.appendChild(div);
        }
        this.els.terminalBody.innerHTML = "";
        this.els.terminalBody.appendChild(frag);
        this.els.terminalBody.scrollTop = this.els.terminalBody.scrollHeight;
    }

    private handleReady(url: string): void {
        // 调试：把实际收到的 URL 输出到日志区和控制台
        console.log("[debug-handleReady] service-ready url:", url);
        this.handleLog(`[调试] handleReady 收到 URL: ${url}`);

        // 切换到 webview-view（空容器，dsh 在弹出的独立窗口中显示）
        this.els.loadingView.classList.add("hidden");
        this.els.webviewView.classList.remove("hidden");

        // Tauri 1.x 不支持嵌入式 webview，必须由 Rust 端 WindowBuilder 创建独立新窗口加载 dsh
        this.handleLog("[系统] 正在弹出 dsh web 窗口...");
        void this.api.openDshWebviewWindow(url)
            .then(() => {
                this.handleLog("[系统] dsh web 窗口已弹出");
            })
            .catch((err: unknown) => {
                const msg = err instanceof Error ? err.message : String(err);
                this.handleLog(`[错误] 弹出 dsh web 窗口失败: ${msg}`);
            });
    }

    private async checkUpdates(): Promise<void> {
        try {
            await this.api.checkForUpdates();
        } catch (err) {
            // 不再静默吞掉：把失败打到日志面板，方便排查「检查更新无反应」
            const msg = err instanceof Error ? err.message : String(err);
            this.handleLog(`[警告] 检查更新失败: ${msg}`);
        }
    }

    private showUpdateDialog(info: ReleaseInfo): void {
        const dialog = document.createElement("div");
        dialog.className = "modal-overlay";
        dialog.innerHTML = `
            <div class="modal-box">
                <div class="modal-title">发现新版本 ${info.version}</div>
                <div class="modal-message">是否前往下载？</div>
                <div class="modal-actions">
                    <button class="btn btn-primary" id="btn-download">下载</button>
                    <button class="btn" id="btn-later">稍后</button>
                </div>
            </div>
        `;
        document.body.appendChild(dialog);

        dialog.querySelector("#btn-download")?.addEventListener("click", async () => {
            await this.api.openUrl(info.html_url);
            dialog.remove();
        });
        dialog.querySelector("#btn-later")?.addEventListener("click", () => dialog.remove());
    }

    /// 检查更新后的统一状态展示：无论「有更新 / 已最新 / 出错」都打一行日志，让用户明确知道结果
    private handleUpdateStatus(r: UpdateCheckResult): void {
        if (r.error) {
            this.handleLog(`[更新] 检查更新失败：${r.error}`);
        } else if (r.update_available) {
            this.handleLog(`[更新] 发现新版本 —— GitHub 最新版本：${r.latest}（当前 ${r.current}）`);
        } else {
            this.handleLog(`[更新] 已是最新 —— GitHub 最新版本：${r.latest}（当前 ${r.current}）`);
        }
    }

    private showNodeMissingDialog(): void {
        const dialog = document.createElement("div");
        dialog.className = "modal-overlay";
        dialog.innerHTML = `
            <div class="modal-box">
                <div class="modal-title">未检测到 Node.js</div>
                <div class="modal-message">启动服务需要 Node.js 环境。<br>请前往官网下载安装。</div>
                <div class="modal-actions">
                    <button class="btn btn-primary" id="btn-nodejs">打开官网</button>
                    <button class="btn" id="btn-close-dialog">关闭</button>
                </div>
            </div>
        `;
        document.body.appendChild(dialog);

        dialog.querySelector("#btn-nodejs")?.addEventListener("click", async () => {
            await this.api.openUrl("https://nodejs.org");
            dialog.remove();
        });
        dialog.querySelector("#btn-close-dialog")?.addEventListener("click", () => dialog.remove());
    }
}
