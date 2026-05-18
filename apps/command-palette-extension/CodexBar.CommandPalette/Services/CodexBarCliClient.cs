using System.Diagnostics;
using System.Text.Json;
using CodexBar.CommandPalette.Models;

namespace CodexBar.CommandPalette.Services;

public sealed class CodexBarCliClient : ICodexBarCliClient
{
    public const string BackendOverrideEnvironmentVariable = "CODEXBAR_BACKEND_EXE";
    private static readonly TimeSpan CliTimeout = TimeSpan.FromSeconds(30);

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    private readonly string _backendPath;

    public CodexBarCliClient()
        : this(ResolveBackendPath())
    {
    }

    public CodexBarCliClient(string backendPath)
    {
        _backendPath = backendPath;
    }

    public static string ResolveBackendPath()
    {
        var overridePath = Environment.GetEnvironmentVariable(BackendOverrideEnvironmentVariable);
        if (!string.IsNullOrWhiteSpace(overridePath))
        {
            return overridePath;
        }

        return Path.Combine(AppContext.BaseDirectory, "Backend", "codexbar.exe");
    }

    public static CmdPalSnapshot ParseSnapshotJson(string json)
    {
        var snapshot = JsonSerializer.Deserialize<CmdPalSnapshot>(json, JsonOptions);
        if (snapshot is null)
        {
            throw new InvalidDataException("codexbar returned an empty Command Palette snapshot.");
        }

        return snapshot;
    }

    public async Task<CmdPalSnapshot> GetSnapshotAsync(CancellationToken cancellationToken)
    {
        if (!File.Exists(_backendPath))
        {
            throw new FileNotFoundException(
                $"CodexBar backend was not found at '{_backendPath}'. Set {BackendOverrideEnvironmentVariable} to a local codexbar.exe while developing.",
                _backendPath);
        }

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(CliTimeout);

        using var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = _backendPath,
                Arguments = "cmdpal snapshot --json",
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
            },
        };

        process.Start();
        var stdoutTask = process.StandardOutput.ReadToEndAsync(timeout.Token);
        var stderrTask = process.StandardError.ReadToEndAsync(timeout.Token);

        try
        {
            await process.WaitForExitAsync(timeout.Token).ConfigureAwait(false);
            var stdout = await stdoutTask.ConfigureAwait(false);
            var stderr = await stderrTask.ConfigureAwait(false);

            if (process.ExitCode != 0)
            {
                var message = string.IsNullOrWhiteSpace(stderr) ? stdout : stderr;
                throw new InvalidOperationException($"codexbar exited with code {process.ExitCode}: {message.Trim()}");
            }

            return ParseSnapshotJson(stdout);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            TryKill(process);
            throw new TimeoutException($"codexbar cmdpal snapshot exceeded {CliTimeout.TotalSeconds:0}s.");
        }
    }

    private static void TryKill(Process process)
    {
        try
        {
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
            }
        }
        catch (InvalidOperationException)
        {
        }
    }
}
