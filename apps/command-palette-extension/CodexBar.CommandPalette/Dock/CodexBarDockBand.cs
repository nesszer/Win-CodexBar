using CodexBar.CommandPalette.Commands;
using CodexBar.CommandPalette.Models;
using CodexBar.CommandPalette.Pages;
using CodexBar.CommandPalette.Services;
using CodexBar.CommandPalette.Ui;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette.Dock;

internal sealed partial class CodexBarDockBand : CommandItem
{
    private readonly CodexBarStateService _state;
    private readonly CodexBarDockHomePage _page;

    public CodexBarDockBand(CodexBarStateService state)
        : this(state, new CodexBarDockHomePage(state))
    {
    }

    private CodexBarDockBand(CodexBarStateService state, CodexBarDockHomePage page)
        : base(page)
    {
        _state = state;
        _page = page;
        Icon = new IconInfo("\uE950");
        MoreCommands =
        [
            new CommandContextItem(new RefreshSnapshotCommand(_state, RefreshDock))
            {
                Title = "Refresh",
            },
            new CommandContextItem(new CodexBarListPage(_state))
            {
                Title = "Open CodexBar Home",
            },
            new CommandContextItem(new OpenCodexBarSettingsCommand())
            {
                Title = "Open CodexBar Settings",
            },
        ];
        RebuildDockText();
    }

    private void RefreshDock()
    {
        _page.RefreshContent();
        RebuildDockText();
    }

    private void RebuildDockText()
    {
        var snapshot = _state.RefreshBlocking();
        var dockProvider = PickDockProvider(snapshot.Providers);
        var dockTitle = dockProvider is null ? "CodexBar" : ProviderText.FiveHourTitle(dockProvider);
        var dockSubtitle = dockProvider is null
            ? DockSubtitle(snapshot.Providers.Count)
            : DockSubtitle(snapshot.Providers.Count, ProviderText.FiveHourSubtitle(dockProvider));

        Title = dockTitle;
        Subtitle = dockSubtitle;
        Icon = dockProvider?.HasError == true ? new IconInfo("\uE783") : new IconInfo("\uE950");
    }

    private string DockSubtitle(int providerCount, string? currentStatus = null)
    {
        if (!string.IsNullOrWhiteSpace(_state.LastError))
        {
            return _state.LastError;
        }

        if (!string.IsNullOrWhiteSpace(currentStatus))
        {
            return currentStatus;
        }

        return providerCount == 0 ? "No enabled providers" : $"{providerCount} enabled providers";
    }

    private static ProviderSnapshot? PickDockProvider(IReadOnlyCollection<ProviderSnapshot> providers)
    {
        return providers
            .Where(provider => provider.Primary is not null && !provider.HasError)
            .OrderByDescending(provider => provider.Primary?.UsedPercent ?? -1)
            .ThenBy(provider => provider.DisplayName, StringComparer.OrdinalIgnoreCase)
            .FirstOrDefault()
            ?? providers
                .OrderByDescending(provider => provider.Primary?.UsedPercent ?? provider.UsedPercent)
                .ThenBy(provider => provider.DisplayName, StringComparer.OrdinalIgnoreCase)
                .FirstOrDefault();
    }
}
