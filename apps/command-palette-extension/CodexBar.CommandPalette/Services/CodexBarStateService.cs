using CodexBar.CommandPalette.Models;

namespace CodexBar.CommandPalette.Services;

public sealed class CodexBarStateService
{
    private static readonly CmdPalSnapshot EmptySnapshot = new()
    {
        ContractVersion = "cmdpal.snapshot.v1",
        GeneratedAt = DateTimeOffset.MinValue,
        RefreshIntervalSecs = 300,
        Providers = [],
    };

    private readonly ICodexBarCliClient _client;
    private readonly SemaphoreSlim _refreshGate = new(1, 1);

    private CmdPalSnapshot _cachedSnapshot = EmptySnapshot;

    public CodexBarStateService(ICodexBarCliClient client)
    {
        _client = client;
    }

    public string? LastError { get; private set; }

    public bool IsRefreshing { get; private set; }

    public CmdPalSnapshot CachedSnapshot => _cachedSnapshot;

    public async Task<CmdPalSnapshot> RefreshAsync(CancellationToken cancellationToken = default)
    {
        if (!await _refreshGate.WaitAsync(0, cancellationToken).ConfigureAwait(false))
        {
            return _cachedSnapshot;
        }

        try
        {
            IsRefreshing = true;
            var snapshot = await _client.GetSnapshotAsync(cancellationToken).ConfigureAwait(false);
            _cachedSnapshot = snapshot;
            LastError = null;
        }
        catch (Exception ex) when (ex is not OperationCanceledException || !cancellationToken.IsCancellationRequested)
        {
            LastError = ex.Message;
        }
        finally
        {
            IsRefreshing = false;
            _refreshGate.Release();
        }

        return _cachedSnapshot;
    }

    public CmdPalSnapshot RefreshBlocking()
    {
        return RefreshAsync().GetAwaiter().GetResult();
    }

    public ProviderSnapshot? FindProvider(string providerId)
    {
        return _cachedSnapshot.Providers.FirstOrDefault(
            provider => string.Equals(provider.ProviderId, providerId, StringComparison.OrdinalIgnoreCase));
    }
}
