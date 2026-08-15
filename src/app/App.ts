import { TauriApi, AppConfig, ReleaseInfo } from "../services/tauri-api";

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
        this.els.btnMinimize.addEventListener("click", () => this.api.minimizeWindow());
        this.els.btnMaximize.addEventListener("click", () => this.api.maximizeWindow());
        this.els.btnClose.addEventListener("click", () => this.api.closeWindow());
    }

    private setupEventListeners(): void {
        this.api.onServiceLog((log) => this.handleLog(log));
        this.api.onServiceReady((url) => this.handleReady(url));
        this.api.onUpdateAvailable((info) => this.showUpdateDialog(info));
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
        // 切换到 webview
        this.els.loadingView.classList.add("hidden");
        this.els.webviewView.classList.remove("hidden");

        const iframe = document.createElement("iframe");
        iframe.src = url;
        iframe.style.border = "none";
        iframe.style.width = "100%";
        iframe.style.height = "100%";
        this.els.webviewView.appendChild(iframe);
    }

    private async checkUpdates(): Promise<void> {
        try {
            await this.api.checkForUpdates();
        } catch {
            // 静默失败
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
