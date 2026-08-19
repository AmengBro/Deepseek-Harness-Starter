import { TauriApi, AppConfig } from "../services/tauri-api";
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

    // 打开 GitHub 项目主页
    btnOpenGithub.addEventListener("click", async () => {
        await api.openUrl("https://github.com/AmengBro/Deepseek-Harness-Starter");
    });
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
