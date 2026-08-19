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

    destroy() {
        this.listeners.forEach((fn) => fn());
        this.listeners = [];
    }
}
