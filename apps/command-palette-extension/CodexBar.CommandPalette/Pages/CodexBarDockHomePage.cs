using CodexBar.CommandPalette.Commands;
using CodexBar.CommandPalette.Models;
using CodexBar.CommandPalette.Services;
using CodexBar.CommandPalette.Ui;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette.Pages;

internal sealed partial class CodexBarDockHomePage : ContentPage
{
    public const string DockCommandId = "com.finesssee.codexbar.commandpalette.dock";

    private readonly CodexBarStateService _state;

    public CodexBarDockHomePage(CodexBarStateService state)
    {
        _state = state;
        Id = DockCommandId;
        Name = "CodexBar";
        Title = "CodexBar";
        Icon = new IconInfo("\uE950");
        Commands =
        [
            new CommandContextItem(new RefreshSnapshotCommand(_state, RefreshContent))
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
    }

    public override IContent[] GetContent()
    {
        var snapshot = _state.RefreshBlocking();
        var providers = snapshot.Providers
            .OrderByDescending(provider => provider.Primary?.UsedPercent ?? -1)
            .ThenByDescending(provider => provider.UsedPercent)
            .ThenBy(provider => provider.DisplayName, StringComparer.OrdinalIgnoreCase)
            .ToList();

        var body = BuildBody(snapshot, providers);
        Details = null;

        return [new MarkdownContent(body)];
    }

    public void RefreshContent()
    {
        RaiseItemsChanged(1);
    }

    private string BuildBody(CmdPalSnapshot snapshot, IReadOnlyCollection<ProviderSnapshot> providers)
    {
        if (!string.IsNullOrWhiteSpace(_state.LastError))
        {
            return $"### CodexBar\n\nRefresh failed: `{_state.LastError}`";
        }

        if (providers.Count == 0)
        {
            return "### CodexBar\n\nNo enabled providers.\n\nUse **Open CodexBar Settings** to configure providers.";
        }

        var primaryProvider = providers.First();
        var fiveHour = primaryProvider.Primary;
        var weekly = primaryProvider.Secondary;
        var reset = fiveHour?.ResetDescription ?? "unknown";
        var source = primaryProvider.Source ?? "unknown";
        var lines = new List<string>
        {
            "## CodexBar",
            string.Empty,
            $"**{primaryProvider.DisplayName}**  5h `{PercentText(fiveHour)}`   Weekly `{PercentText(weekly)}`",
            string.Empty,
        };

        if (fiveHour is not null)
        {
            lines.Add($"`{ProgressBar(fiveHour.UsedPercent)}`");
            lines.Add($"{fiveHour.RemainingPercent:0}% remaining  -  reset {reset}");
            lines.Add(string.Empty);
        }

        lines.Add($"Updated `{FormatTime(snapshot.GeneratedAt)}`  -  source `{source}`");
        lines.Add(string.Empty);

        foreach (var provider in providers.Take(3))
        {
            lines.Add(ProviderCard(provider));
            lines.Add(string.Empty);
        }

        return string.Join(Environment.NewLine, lines).TrimEnd();
    }

    private static string ProviderCard(ProviderSnapshot provider)
    {
        var fiveHour = PercentText(provider.Primary);
        var weekly = PercentText(provider.Secondary);
        var reset = provider.Primary?.ResetDescription ?? "-";
        var status = provider.HasError ? "error" : "ok";

        return string.Join(Environment.NewLine, [
            $"### {provider.DisplayName}",
            $"5h `{fiveHour}`   Weekly `{weekly}`   Reset `{reset}`   `{status}`",
        ]);
    }

    private static string PercentText(RateWindowSnapshot? window)
    {
        return window is null ? "-" : $"{window.UsedPercent:0}%";
    }

    private static string ProgressBar(double usedPercent)
    {
        var clamped = Math.Clamp(usedPercent, 0, 100);
        var filled = (int)Math.Round(clamped / 5, MidpointRounding.AwayFromZero);
        filled = Math.Clamp(filled, 0, 20);
        return $"[{new string('=', filled)}{new string('-', 20 - filled)}] {clamped:0}%";
    }

    private static string FormatTime(DateTimeOffset? value)
    {
        return value?.ToString("HH:mm:ss") ?? "unknown";
    }
}
