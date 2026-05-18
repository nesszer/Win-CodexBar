using CodexBar.CommandPalette.Commands;
using CodexBar.CommandPalette.Services;
using CodexBar.CommandPalette.Ui;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette.Pages;

internal sealed partial class CodexBarListPage : ListPage
{
    private readonly CodexBarStateService _state;

    public CodexBarListPage(CodexBarStateService state)
    {
        _state = state;
        Name = "CodexBar";
        Icon = new IconInfo("\uE950");
    }

    public override IListItem[] GetItems()
    {
        var snapshot = _state.RefreshBlocking();
        var items = snapshot.Providers
            .OrderByDescending(provider => provider.UsedPercent)
            .Select(provider => new ListItem(new ProviderDetailPage(_state, provider.ProviderId))
            {
                Title = ProviderText.Title(provider),
                Subtitle = ProviderText.Subtitle(provider),
                Icon = provider.HasError ? new IconInfo("\uE783") : new IconInfo("\uE9D9"),
                MoreCommands = ProviderCommands(provider),
            })
            .Cast<IListItem>()
            .ToList();

        if (items.Count == 0)
        {
            items.Add(new ListItem(new NoOpCommand())
            {
                Title = "No enabled providers",
                Subtitle = string.IsNullOrWhiteSpace(_state.LastError)
                    ? "Enable providers in CodexBar settings."
                    : _state.LastError,
                Icon = new IconInfo("\uE946"),
            });
        }

        items.Add(new ListItem(new RefreshSnapshotCommand(_state))
        {
            Title = "Refresh",
            Subtitle = "Reload the read-only snapshot from codexbar.exe",
            Icon = new IconInfo("\uE72C"),
        });
        items.Add(new ListItem(new OpenCodexBarSettingsCommand())
        {
            Title = "Open CodexBar Settings",
            Subtitle = "Launch the desktop app for provider credentials and preferences",
            Icon = new IconInfo("\uE713"),
        });

        return items.ToArray();
    }

    private static IContextItem[] ProviderCommands(Models.ProviderSnapshot provider)
    {
        var commands = new List<IContextItem>();

        if (!string.IsNullOrWhiteSpace(provider.DashboardUrl))
        {
            commands.Add(new CommandContextItem(new OpenUrlCommand(provider.DashboardUrl))
            {
                Title = "Open Dashboard",
            });
        }

        if (!string.IsNullOrWhiteSpace(provider.StatusPageUrl))
        {
            commands.Add(new CommandContextItem(new OpenUrlCommand(provider.StatusPageUrl))
            {
                Title = "Open Status Page",
            });
        }

        return commands.ToArray();
    }
}
