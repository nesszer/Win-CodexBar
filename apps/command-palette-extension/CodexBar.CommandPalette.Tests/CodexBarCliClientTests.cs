using CodexBar.CommandPalette.Models;
using CodexBar.CommandPalette.Services;
using CodexBar.CommandPalette.Ui;

namespace CodexBar.CommandPalette.Tests;

[TestClass]
public sealed class CodexBarCliClientTests
{
    [TestMethod]
    public void ParseSnapshotJson_reads_provider_usage()
    {
        const string Json = """
            {
              "contractVersion": "cmdpal.snapshot.v1",
              "generatedAt": "2026-05-18T12:00:00Z",
              "refreshIntervalSecs": 300,
              "providers": [
                {
                  "providerId": "codex",
                  "displayName": "Codex",
                  "primaryLabel": "Session",
                  "primary": {
                    "usedPercent": 72.4,
                    "remainingPercent": 27.6,
                    "windowMinutes": 300,
                    "resetsAt": "2026-05-18T14:00:00Z",
                    "resetDescription": "2h",
                    "isExhausted": false
                  },
                  "source": "oauth",
                  "updatedAt": "2026-05-18T12:00:00Z",
                  "dashboardUrl": "https://chatgpt.com/codex/settings/usage"
                }
              ]
            }
            """;

        var snapshot = CodexBarCliClient.ParseSnapshotJson(Json);

        Assert.AreEqual("cmdpal.snapshot.v1", snapshot.ContractVersion);
        Assert.AreEqual(300, snapshot.RefreshIntervalSecs);
        Assert.AreEqual(1, snapshot.Providers.Count);
        Assert.AreEqual("codex", snapshot.Providers[0].ProviderId);
        Assert.AreEqual(72.4, snapshot.Providers[0].UsedPercent, 0.0001);
        Assert.AreEqual("2h", snapshot.Providers[0].Primary?.ResetDescription);
    }

    [TestMethod]
    public void ResolveBackendPath_uses_environment_override()
    {
        var previous = Environment.GetEnvironmentVariable(CodexBarCliClient.BackendOverrideEnvironmentVariable);
        try
        {
            Environment.SetEnvironmentVariable(CodexBarCliClient.BackendOverrideEnvironmentVariable, @"C:\dev\codexbar.exe");

            Assert.AreEqual(@"C:\dev\codexbar.exe", CodexBarCliClient.ResolveBackendPath());
        }
        finally
        {
            Environment.SetEnvironmentVariable(CodexBarCliClient.BackendOverrideEnvironmentVariable, previous);
        }
    }

    [TestMethod]
    public async Task StateService_retains_previous_cache_after_refresh_failure()
    {
        var snapshot = new CmdPalSnapshot
        {
            ContractVersion = "cmdpal.snapshot.v1",
            GeneratedAt = DateTimeOffset.UtcNow,
            RefreshIntervalSecs = 300,
            Providers = [new ProviderSnapshot { ProviderId = "codex", DisplayName = "Codex" }],
        };
        var client = new QueueClient(snapshot, new InvalidOperationException("boom"));
        var service = new CodexBarStateService(client);

        var first = await service.RefreshAsync();
        var second = await service.RefreshAsync();

        Assert.AreSame(snapshot, first);
        Assert.AreSame(snapshot, second);
        Assert.AreSame(snapshot, service.CachedSnapshot);
        Assert.AreEqual("boom", service.LastError);
    }

    [TestMethod]
    public async Task StateService_does_not_start_parallel_refreshes()
    {
        var client = new BlockingClient();
        var service = new CodexBarStateService(client);

        var first = service.RefreshAsync();
        await client.Started.Task;
        var second = await service.RefreshAsync();
        client.Release.SetResult();
        await first;

        Assert.AreEqual(1, client.CallCount);
        Assert.AreEqual(0, second.Providers.Count);
    }

    [TestMethod]
    public void ProviderText_five_hour_title_uses_primary_window()
    {
        var provider = new ProviderSnapshot
        {
            ProviderId = "codex",
            DisplayName = "Codex",
            Primary = new RateWindowSnapshot { UsedPercent = 24.6, ResetDescription = "1h 20m" },
            Secondary = new RateWindowSnapshot { UsedPercent = 91.2, ResetDescription = "tomorrow" },
        };

        Assert.AreEqual("Codex 5h 25%", ProviderText.FiveHourTitle(provider));
        Assert.AreEqual("5h reset 1h 20m", ProviderText.FiveHourSubtitle(provider));
    }

    private sealed class QueueClient : ICodexBarCliClient
    {
        private readonly Queue<object> _responses;

        public QueueClient(params object[] responses)
        {
            _responses = new Queue<object>(responses);
        }

        public Task<CmdPalSnapshot> GetSnapshotAsync(CancellationToken cancellationToken)
        {
            var next = _responses.Dequeue();
            if (next is Exception ex)
            {
                throw ex;
            }

            return Task.FromResult((CmdPalSnapshot)next);
        }
    }

    private sealed class BlockingClient : ICodexBarCliClient
    {
        public TaskCompletionSource Started { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        public TaskCompletionSource Release { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        public int CallCount { get; private set; }

        public async Task<CmdPalSnapshot> GetSnapshotAsync(CancellationToken cancellationToken)
        {
            CallCount++;
            Started.SetResult();
            await Release.Task.WaitAsync(cancellationToken);
            return new CmdPalSnapshot
            {
                ContractVersion = "cmdpal.snapshot.v1",
                GeneratedAt = DateTimeOffset.UtcNow,
                RefreshIntervalSecs = 300,
                Providers = [new ProviderSnapshot { ProviderId = "codex", DisplayName = "Codex" }],
            };
        }
    }
}
