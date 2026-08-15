using Microsoft.Win32;

namespace DeepseekHarness.Setup.Services;

public static class RegistryHelper
{
    const string UninstallKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\DeepseekHarness";

    public static void WriteUninstallInfo(string folder, string version, long size)
    {
        string exe = Path.Combine(folder, "DeepseekHarness.exe");
        string setupExe = Path.Combine(folder, "DeepseekHarness.Setup.exe");
        using var subkey = Registry.CurrentUser.CreateSubKey(UninstallKey);
        subkey.SetValue("Publisher", "AmengBro", RegistryValueKind.String);
        subkey.SetValue("DisplayName", "DeepseekHarness", RegistryValueKind.String);
        subkey.SetValue("DisplayIcon", exe, RegistryValueKind.String);
        subkey.SetValue("DisplayVersion", version, RegistryValueKind.String);
        subkey.SetValue("InstallLocation", folder, RegistryValueKind.String);
        subkey.SetValue("EstimatedSize", (int)(size / 1024), RegistryValueKind.DWord);
        subkey.SetValue("InstallDate", $"{DateTime.Now:yyyyMMdd}", RegistryValueKind.String);
        subkey.SetValue("UninstallString", $"\"{setupExe}\" uninstall", RegistryValueKind.String);
        subkey.SetValue("QuietUninstallString", $"\"{setupExe}\" uninstall /S", RegistryValueKind.String);
    }

    public static void DeleteUninstallInfo()
    {
        Registry.CurrentUser.DeleteSubKeyTree(UninstallKey, false);
    }

    public static string? GetInstallLocation()
    {
        using var subkey = Registry.CurrentUser.OpenSubKey(UninstallKey);
        return subkey?.GetValue("InstallLocation") as string;
    }
}
