import { invoke } from "@tauri-apps/api/tauri";
import { listen } from "@tauri-apps/api/event";
import { open as shellOpen } from "@tauri-apps/api/shell";
import type { UnlistenFn } from "@tauri-apps/api/event";

export interface AppConfig {
    port: number;
    theme: string;
}

export interface ServiceStatus {
    running: boolean;
    port: number;
    url: string;
}

export interface ReleaseInfo {
    version: string;
    html_url: string;
    published_at?: string;
}

export interface DshVersionInfo {
    current: string;
    latest: string;
    needs_update: boolean;
    source: string;
}

export interface McpServerEntry {
    id: string;
    server_name: string;
    transport: string;
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    url?: string;
    headers?: Record<string, string>;
}

export interface ExtensionInfo {
    name: string;
    version: string;
}

export interface McpServerInput {
    id?: string;
    server_name: string;
    transport: string;
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    url?: string;
    headers?: Record<string, string>;
}

export class TauriApi {
    private listeners: UnlistenFn[] = [];

    async loadConfig(): Promise<AppConfig> {
        return invoke<AppConfig>("load_config");
    }

    async saveConfig(config: AppConfig): Promise<void> {
        return invoke<void>("save_config", { config });
    }

    async startService(config: AppConfig): Promise<void> {
        return invoke<void>("start_harness_service", { config });
    }

    async stopService(): Promise<void> {
        return invoke<void>("stop_harness_service");
    }

    async getServiceStatus(): Promise<ServiceStatus> {
        return invoke<ServiceStatus>("get_service_status");
    }

    async checkNode(): Promise<string> {
        return invoke<string>("check_nodejs");
    }

    async checkForUpdates(): Promise<ReleaseInfo | null> {
        return invoke<ReleaseInfo | null>("check_for_updates");
    }

    async checkDshVersion(): Promise<DshVersionInfo> {
        return invoke<DshVersionInfo>("check_dsh_version");
    }

    async updateDsh(): Promise<string> {
        return invoke<string>("update_dsh");
    }

    async openSettings(): Promise<void> {
        return invoke<void>("open_settings_window");
    }

    async openLogFolder(): Promise<void> {
        return invoke<void>("open_log_folder");
    }

    async openInstallDir(): Promise<void> {
        return invoke<void>("open_install_dir");
    }

    async openSkillsFolder(): Promise<void> {
        return invoke<void>("open_skills_folder");
    }

    async openUrl(url: string): Promise<void> {
        await shellOpen(url);
    }

    async minimizeWindow(): Promise<void> {
        const { getCurrent } = await import("@tauri-apps/api/window");
        const window = getCurrent();
        await window.minimize();
    }

    async closeWindow(): Promise<void> {
        const { getCurrent } = await import("@tauri-apps/api/window");
        const window = getCurrent();
        await window.hide();
    }

    async maximizeWindow(): Promise<void> {
        const { getCurrent } = await import("@tauri-apps/api/window");
        const window = getCurrent();
        const isMaximized = await window.isMaximized();
        if (isMaximized) {
            await window.unmaximize();
        } else {
            await window.maximize();
        }
    }

    async onServiceLog(callback: (log: string) => void): Promise<void> {
        const unlisten = await listen<string>("service-log", (event) => {
            callback(event.payload);
        });
        this.listeners.push(unlisten);
    }

    async onServiceStatus(callback: (status: ServiceStatus) => void): Promise<void> {
        const unlisten = await listen<ServiceStatus>("service-status", (event) => {
            callback(event.payload);
        });
        this.listeners.push(unlisten);
    }

    async onServiceReady(callback: (url: string) => void): Promise<void> {
        const unlisten = await listen<string>("service-ready", (event) => {
            callback(event.payload);
        });
        this.listeners.push(unlisten);
    }

    async onUpdateAvailable(callback: (info: ReleaseInfo) => void): Promise<void> {
        const unlisten = await listen<ReleaseInfo>("update-available", (event) => {
            callback(event.payload);
        });
        this.listeners.push(unlisten);
    }

    async onDshUpdateLog(callback: (log: string) => void): Promise<void> {
        const unlisten = await listen<string>("dsh-update-log", (event) => {
            callback(event.payload);
        });
        this.listeners.push(unlisten);
    }

    async onExtensionLog(callback: (log: string) => void): Promise<void> {
        const unlisten = await listen<string>("extension-log", (event) => {
            callback(event.payload);
        });
        this.listeners.push(unlisten);
    }

    // ───── MCP 管理 ─────
    async getMcpConfigPath(): Promise<{ path: string; exists: boolean }> {
        return invoke<{ path: string; exists: boolean }>("get_mcp_config_path_cmd");
    }

    async listMcpServers(): Promise<McpServerEntry[]> {
        return invoke<McpServerEntry[]>("list_mcp_servers");
    }

    async addMcpServer(input: McpServerInput): Promise<string> {
        return invoke<string>("add_mcp_server", { input });
    }

    async removeMcpServer(id: string): Promise<void> {
        return invoke<void>("remove_mcp_server", { id });
    }

    async openMcpConfigFile(): Promise<void> {
        return invoke<void>("open_mcp_config_file");
    }

    // ───── 扩展（插件）管理 ─────
    async ensureDeps(): Promise<void> {
        return invoke<void>("ensure_deps");
    }

    async installExtension(source: string): Promise<void> {
        return invoke<void>("install_extension", { source });
    }

    async listExtensions(): Promise<ExtensionInfo[]> {
        return invoke<ExtensionInfo[]>("list_extensions");
    }

    async uninstallExtension(pkg: string): Promise<void> {
        return invoke<void>("uninstall_extension", { pkg });
    }

    destroy() {
        this.listeners.forEach((fn) => fn());
        this.listeners = [];
    }
}
