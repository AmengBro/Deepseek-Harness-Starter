namespace DeepseekHarness.Setup.Services;

public static class DriveHelper
{
    public static string GetSizeText(long size)
    {
        const double KB = 1 << 10;
        const double MB = 1 << 20;
        const double GB = 1 << 30;
        return size switch
        {
            >= (1 << 30) => $"{size / GB:F2} GB",
            >= (1 << 20) => $"{size / MB:F2} MB",
            _ => $"{size / KB:F2} KB",
        };
    }

    public static string GetDriveAvailableSpaceText(string path)
    {
        try
        {
            var drive = new DriveInfo(path);
            if (drive.IsReady)
            {
                return GetSizeText(drive.AvailableFreeSpace);
            }
        }
        catch { }
        return "-";
    }
}
