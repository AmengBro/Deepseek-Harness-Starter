namespace DeepseekHarness.Setup;

public static class Lang
{
    public const string AppName = "DeepseekHarness";
    public const string SetupTitle = "DeepseekHarness 安装";
    public const string UninstallerTitle = "DeepseekHarness 卸载";
    public const string Description = "DeepSeek Harness 一键启动工具";

    public const string OSVersionTooOld = "DeepseekHarness 需要 Windows 10 1809（版本 17763）或更高版本。";
    public const string SetupAlreadyRunning = "安装程序已在运行，请勿重复启动。";

    public const string Change = "更改";
    public const string SpaceRequired = "需要空间";
    public const string SpaceAvailable = "可用空间";
    public const string Install = "安装";
    public const string Launch = "启动";
    public const string Uninstall = "卸载";
    public const string UninstallationComplete = "卸载完成";

    public const string AppIsRunningForceClose = "DeepseekHarness 正在运行（进程 ID：{0}），是否强制结束并继续？";
    public const string InstallFailed = "安装过程中发生错误";
    public const string UninstallFailed = "卸载过程中发生错误";
    public const string InstallPathCaution = "未找到有效的安装目录，无法卸载。";
    public const string ConfigDataCaution = "● 安装目录下的 config 文件夹包含端口配置与应用日志";
    public const string AllFilesCanBeSafelyDeleted = "所有文件均可安全删除。";
    public const string IAcknowledgeTheAboveRisks = "我了解上述风险，确认卸载";
    public const string KeepConfigData = "保留配置文件与日志";
}
