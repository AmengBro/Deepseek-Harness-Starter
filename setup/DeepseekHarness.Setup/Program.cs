using Aprillz.MewUI;
using DeepseekHarness.Setup;
using DeepseekHarness.Setup.Views;
using System.Diagnostics;

Win32Platform.Register();
Direct2DBackend.Register();

if (args.Length > 0 && args[0] is "uninstall-clear")
Win32Platform.Register();
Direct2DBackend.Register();

if (args.Length > 0 && args[0] is "uninstall-clear")
{
    int pid = int.Parse(args[1]);
    string folder = args[2];
    var p = Process.GetProcessById(pid);
    while (true)
    {
        await Task.Delay(100);
        if (p.HasExited)
        {
            break;
        }
    }
    await Task.Delay(1000);
    try
    {
        Directory.Delete(folder, true);
    }
    catch { }

    Process.Start(new ProcessStartInfo
    {
        FileName = "cmd",
        Arguments = $"""
        /C ping 127.0.0.1 -n 3 >nul & del /f /q "{Environment.ProcessPath}"
        """,
        CreateNoWindow = true,
    });
}
else if (args.Length > 0 && args[0] == "uninstall")
{
    CheckMutex();
    Application.Create().UseAccent(Accent.Blue).Run(new UninstallWindow());
}
else if (Environment.OSVersion.Version < new Version("10.0.17763"))
{
    Application.Create().UseAccent(Accent.Blue).Run(new TextWindow(Lang.OSVersionTooOld));
}
else
{
    CheckMutex();
    Application.Create().UseAccent(Accent.Blue).Run(new InstallWindow());
}

Mutex mutex;

void CheckMutex()
{
    mutex = new Mutex(true, "DeepseekHarness.Setup", out bool createdNew);
    if (!createdNew)
    {
        Application.Create().UseAccent(Accent.Blue).Run(new TextWindow(Lang.SetupAlreadyRunning));
        Environment.Exit(0);
    }
}
