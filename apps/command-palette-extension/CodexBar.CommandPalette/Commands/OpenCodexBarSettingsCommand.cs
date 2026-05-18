using System.Diagnostics;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette.Commands;

internal sealed partial class OpenCodexBarSettingsCommand : InvokableCommand
{
    private const string AppOverrideEnvironmentVariable = "CODEXBAR_APP_EXE";

    public OpenCodexBarSettingsCommand()
    {
        Name = "Open CodexBar Settings";
        Icon = new IconInfo("\uE713");
    }

    public override ICommandResult Invoke()
    {
        var appPath = ResolveAppPath();
        if (appPath is null)
        {
            return CommandResult.ShowToast(
                $"CodexBar desktop app was not found. Set {AppOverrideEnvironmentVariable} to CodexBar.exe or install the Tauri app.");
        }

        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = appPath,
                UseShellExecute = true,
            });
            return CommandResult.GoHome();
        }
        catch (Exception ex)
        {
            return CommandResult.ShowToast($"Could not open CodexBar: {ex.Message}");
        }
    }

    private static string? ResolveAppPath()
    {
        var overridePath = Environment.GetEnvironmentVariable(AppOverrideEnvironmentVariable);
        if (!string.IsNullOrWhiteSpace(overridePath) && File.Exists(overridePath))
        {
            return overridePath;
        }

        foreach (var candidate in CandidatePaths())
        {
            if (File.Exists(candidate))
            {
                return candidate;
            }
        }

        return null;
    }

    private static IEnumerable<string> CandidatePaths()
    {
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var programFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        var programFilesX86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);

        if (!string.IsNullOrWhiteSpace(localAppData))
        {
            yield return Path.Combine(localAppData, "Programs", "CodexBar", "CodexBar.exe");
            yield return Path.Combine(localAppData, "CodexBar", "CodexBar.exe");
        }

        if (!string.IsNullOrWhiteSpace(programFiles))
        {
            yield return Path.Combine(programFiles, "CodexBar", "CodexBar.exe");
        }

        if (!string.IsNullOrWhiteSpace(programFilesX86))
        {
            yield return Path.Combine(programFilesX86, "CodexBar", "CodexBar.exe");
        }
    }
}
