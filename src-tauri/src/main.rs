#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 真正的入口实现位于 lib.rs（run()），
    // 包含托盘菜单、设置窗口、服务管理、配置与全部命令注册。
    deepseek_harness::run();
}
