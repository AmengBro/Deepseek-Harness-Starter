using Aprillz.MewUI;
using Aprillz.MewUI.Controls;
using DeepseekHarness.Setup.Services;
using System.Diagnostics;
using System.Reflection;
using WindowsShortcutFactory;

namespace DeepseekHarness.Setup.Views;

public class InstallWindow : WindowBase
{
    const int WindowWidth = 660;
    const int WindowHeight = 400;

    const string AppFileName = "DeepseekHarness.exe";
    const string SetupFileName = "DeepseekHarness.Setup.exe";
    const string InstallFolderFlag = "DeepseekHarnessInstallFolder";
    const string PayloadResource = "Payload.DeepseekHarness.exe";
    const string LogoResource = "Assets.AppLogo.png";

    private readonly ObservableValue<string> InstallFolder = new(" ");
    private readonly ObservableValue<string> DriveAvailableSizeText = new("-");
    private readonly ObservableValue<string> InstallSizeText = new("-");
    private readonly ObservableValue<double> ProgressBarValue = new(0);
    private readonly ObservableValue<string> InstallProgressText = new(" ");

    private TextBlock TextBlock_TitleHeader = null!;
    private StackPanel StackPanel_InstallInfo = null!;
    private StackPanel StackPanel_InstallProgress = null!;
    private Button Button_Launch = null!;

    public InstallWindow() : base()
    {
        this.Resizable(WindowWidth, WindowHeight);
        this.Title = Lang.SetupTitle;
        BuildUI();
    }

    private void BuildUI()
    {
        this.Content = new Grid().Rows("1*,1*").Children(
             new TextBlock().Ref(out TextBlock_TitleHeader)
                            .Row(0)
                            .Top()
                            .Left()
                            .FontSize(13)
                            .Text(Lang.SetupTitle)
                            .Foreground(Theme.Palette.DisabledAccent)
                            .Margin(12, 8, 0, 0),
             BuildHeader(),
             BuildInstallInfo().Row(1),
             BuildInstallProgress().Row(1).IsVisible(false),
             new Button().Ref(out Button_Launch)
                         .OnClick(Launch)
                         .FontSize(14)
                         .Padding(20, 10, 20, 10)
                         .Center()
                         .Content(Lang.Launch)
                         .BorderThickness(0)
                         .Row(1)
                         .IsVisible(false)
          );
    }

    private UIElement BuildHeader()
    {
        return new StackPanel().Row(0)
                               .Center()
                               .Horizontal()
                               .Spacing(24)
                               .Children(
            new Image().Center()
                       .Size(120)
                       .ImageScaleQuality(ImageScaleQuality.HighQuality)
                       .SourceResource<InstallWindow>(LogoResource),
            new StackPanel().CenterVertical()
                            .Spacing(4)
                            .Children(
                new TextBlock().FontWeight(FontWeight.Bold)
                               .FontSize(28)
                               .Foreground(Theme.Palette.Accent)
                               .Text(Lang.AppName),
                new TextBlock().FontSize(14)
                               .Foreground(Theme.Palette.DisabledAccent)
                               .Text(Lang.Description)));
    }

    private UIElement BuildInstallInfo()
    {
        return new StackPanel().Ref(out StackPanel_InstallInfo)
                               .CenterHorizontal()
                               .Top()
                               .Spacing(8)
                               .Children(
            new Border().Margin(12, 0, 12, 0)
                        .CornerRadius(8)
                        .BorderThickness(1)
                        .MinWidth(440)
                        .Height(36)
                        .BorderBrush(Theme.Palette.PlaceholderText)
                        .Child(
                new Grid().Columns("*,Auto").Children(
                    new TextBlock().TextWrapping(TextWrapping.Wrap)
                                   .BindText(InstallFolder)
                                   .FontSize(12)
                                   .Margin(8, 0, 8, 0),
                    new Button().Content(Lang.Change)
                                .BorderThickness(0)
                                .FontSize(13)
                                .OnClick(ChangeInstallFolder))),
            new StackPanel().Margin(12, 0, 12, 0)
                            .Horizontal()
                            .Spacing(4)
                            .Children(
                new TextBlock().CenterVertical().Text(Lang.SpaceRequired).WithTheme((t, c) => c.Foreground = t.Palette.DisabledText),
                new TextBlock().CenterVertical().BindText(InstallSizeText).WithTheme((t, c) => c.Foreground = t.Palette.DisabledText),
                new TextBlock().CenterVertical().Text(Lang.SpaceAvailable).Margin(4, 0, 0, 0).WithTheme((t, c) => c.Foreground = t.Palette.DisabledText),
                new TextBlock().CenterVertical().BindText(DriveAvailableSizeText).WithTheme((t, c) => c.Foreground = t.Palette.DisabledText)),
            new Button().Content(Lang.Install)
                        .OnClick(StartInstall)
                        .CenterHorizontal()
                        .BorderThickness(0)
                        .FontSize(15)
                        .Padding(20, 10, 20, 10)
                        .Margin(0, 20, 0, 0));
    }

    private UIElement BuildInstallProgress()
    {
        return new StackPanel().Ref(out StackPanel_InstallProgress)
                               .Center()
                               .Spacing(12)
                               .Children(
            new TextBlock().BindText(InstallProgressText)
                           .FontSize(13)
                           .CenterHorizontal()
                           .Foreground(Theme.Palette.DisabledText),
            new ProgressBar().Width(400)
                             .Height(20)
                             .BindValue(ProgressBarValue));
    }

    protected override void OnLoaded()
    {
        base.OnLoaded();
        SetDefaultInstallFolder();
        InstallSizeText.Value = DriveHelper.GetSizeText(GetPayloadSize());
        TextBlock_TitleHeader.Text = $"{Lang.SetupTitle}  ·  {AppVersion}";
        if (SilentMode)
        {
            StartInstall();
        }
    }

    private static string AppVersion => Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "1.0.3";

    private void SetDefaultInstallFolder()
    {
        try
        {
            string? folder = RegistryHelper.GetInstallLocation();
            if (!Directory.Exists(folder) || !string.Equals(new DirectoryInfo(folder).Name, "DeepseekHarness", StringComparison.OrdinalIgnoreCase))
            {
                folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "DeepseekHarness");
            }
            SetInstallFolder(folder);
        }
        catch
        {
            SetInstallFolder(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "DeepseekHarness"));
        }
    }

    private void ChangeInstallFolder()
    {
        try
        {
            string? folder = FileDialog.SelectFolder(new FolderDialogOptions { Owner = this });
            if (Directory.Exists(folder))
            {
                if (new DirectoryInfo(folder).Name is not "DeepseekHarness")
                {
                    folder = Path.Combine(folder, "DeepseekHarness");
                }
                SetInstallFolder(folder);
            }
        }
        catch (Exception ex)
        {
            Debug.WriteLine(ex);
        }
    }

    private void SetInstallFolder(string folder)
    {
        try
        {
            InstallFolder.Value = folder;
            DriveAvailableSizeText.Value = DriveHelper.GetDriveAvailableSpaceText(InstallFolder.Value);
        }
        catch { }
    }

    private static long GetPayloadSize()
    {
        var assembly = Assembly.GetExecutingAssembly();
        using var stream = assembly.GetManifestResourceStream(PayloadResource);
        return stream?.Length ?? 0;
    }

    private async void StartInstall()
    {
        const double MB = 1 << 20;
        try
        {
            if (!string.Equals(new DirectoryInfo(InstallFolder.Value).Name, "DeepseekHarness", StringComparison.OrdinalIgnoreCase))
            {
                if (SilentMode)
                {
                    Console.Error.WriteLine("Silent install failed: invalid install folder.");
                    Environment.Exit(1);
                }
                return;
            }

            if (!await CheckProcessAsync(InstallFolder.Value))
            {
                return;
            }

            ChangeState(InstallState.Installing);

            long total = GetPayloadSize();
            await ExtractPayloadAsync(InstallFolder.Value, new Progress<(long copied, long total)>(p =>
            {
                ProgressBarValue.Value = total == 0 ? 0 : p.copied * 100d / total;
                InstallProgressText.Value = $"{p.copied / MB:F2}/{p.total / MB:F2} MB";
            }));

            File.Copy(Environment.ProcessPath!, Path.Combine(InstallFolder.Value, SetupFileName), true);
            await File.WriteAllTextAsync(Path.Combine(InstallFolder.Value, InstallFolderFlag), InstallFolder.Value);
            RegistryHelper.WriteUninstallInfo(InstallFolder.Value, AppVersion, total);
            CreateShortcut();
            ChangeState(InstallState.Finished);
            if (SilentMode)
            {
                Environment.Exit(0);
            }
        }
        catch (Exception ex)
        {
            ChangeState(InstallState.None);
            if (SilentMode)
            {
                Console.Error.WriteLine($"Silent install failed: {ex}");
                Environment.Exit(1);
            }
            else
            {
                await MessageBox.NotifyAsync($"{Lang.InstallFailed}:\n{ex.Message}", PromptIconKind.Error, owner: this);
            }
        }
    }

    private static async Task ExtractPayloadAsync(string folder, IProgress<(long copied, long total)> progress)
    {
        Directory.CreateDirectory(folder);
        var assembly = Assembly.GetExecutingAssembly();
        await using var stream = assembly.GetManifestResourceStream(PayloadResource) ?? throw new InvalidOperationException("未找到安装负载资源");
        string target = Path.Combine(folder, AppFileName);
        await using var fs = new FileStream(target, FileMode.Create, FileAccess.Write, FileShare.None, 1 << 20, useAsync: true);
        byte[] buffer = new byte[1 << 20];
        long total = stream.Length;
        long copied = 0;
        int read;
        while ((read = await stream.ReadAsync(buffer)) > 0)
        {
            await fs.WriteAsync(buffer.AsMemory(0, read));
            copied += read;
            progress.Report((copied, total));
        }
    }

    private void ChangeState(InstallState state)
    {
        switch (state)
        {
            case InstallState.None:
                StackPanel_InstallInfo.IsVisible = true;
                StackPanel_InstallProgress.IsVisible = false;
                Button_Launch.IsVisible = false;
                break;
            case InstallState.Installing:
                StackPanel_InstallInfo.IsVisible = false;
                StackPanel_InstallProgress.IsVisible = true;
                Button_Launch.IsVisible = false;
                break;
            case InstallState.Finished:
                StackPanel_InstallInfo.IsVisible = false;
                StackPanel_InstallProgress.IsVisible = false;
                Button_Launch.IsVisible = true;
                break;
            default:
                break;
        }
    }

    private enum InstallState
    {
        None = 0,
        Installing = 1,
        Finished = 2,
    }

    private void Launch()
    {
        try
        {
            string exe = Path.Combine(InstallFolder.Value, AppFileName);
            if (File.Exists(exe))
            {
                Process.Start("explorer", $"\"{exe}\"");
                Environment.Exit(0);
            }
            else
            {
                StartInstall();
            }
        }
        catch { }
    }

    private void CreateShortcut()
    {
        string folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "DeepseekHarness");
        Directory.CreateDirectory(folder);
        CreateShortcutInternal(Path.Combine(folder, "DeepseekHarness.lnk"));
        try
        {
            CreateShortcutInternal(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Desktop), "DeepseekHarness.lnk"));
        }
        catch { }
    }

    private void CreateShortcutInternal(string shortcutPath)
    {
        using var shortcut = new WindowsShortcut
        {
            Path = Path.Combine(InstallFolder.Value, AppFileName),
            WorkingDirectory = InstallFolder.Value,
        };
        shortcut.Save(shortcutPath);
    }
}
