using CodexBar.CommandPalette.Services;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette.Commands;

internal sealed partial class RefreshSnapshotCommand : InvokableCommand
{
    private readonly CodexBarStateService _state;
    private readonly Action? _afterRefresh;

    public RefreshSnapshotCommand(CodexBarStateService state, Action? afterRefresh = null)
    {
        _state = state;
        _afterRefresh = afterRefresh;
        Name = "Refresh";
        Icon = new IconInfo("\uE72C");
    }

    public override ICommandResult Invoke()
    {
        _state.RefreshBlocking();
        _afterRefresh?.Invoke();

        return string.IsNullOrWhiteSpace(_state.LastError)
            ? CommandResult.ShowToast("CodexBar refreshed")
            : CommandResult.ShowToast($"CodexBar refresh failed: {_state.LastError}");
    }
}
