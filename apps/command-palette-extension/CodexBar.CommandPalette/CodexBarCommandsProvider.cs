using CodexBar.CommandPalette.Commands;
using CodexBar.CommandPalette.Dock;
using CodexBar.CommandPalette.Pages;
using CodexBar.CommandPalette.Services;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette;

public sealed partial class CodexBarCommandsProvider : CommandProvider
{
    public const string ProviderId = "com.finesssee.codexbar.commandpalette";

    private readonly CodexBarStateService _state = new(new CodexBarCliClient());
    private readonly ICommandItem[] _commands;

    public CodexBarCommandsProvider()
    {
        DisplayName = "CodexBar";
        Id = ProviderId;
        Icon = new IconInfo("\uE950");

        var page = new CodexBarListPage(_state);
        _commands =
        [
            new CommandItem(page)
            {
                Title = "CodexBar",
                Subtitle = "AI provider usage snapshot",
                Icon = Icon,
                MoreCommands =
                [
                    new CommandContextItem(new RefreshSnapshotCommand(_state))
                    {
                        Title = "Refresh",
                    },
                    new CommandContextItem(new OpenCodexBarSettingsCommand())
                    {
                        Title = "Open CodexBar Settings",
                    },
                ],
            },
        ];
    }

    public override ICommandItem[] TopLevelCommands()
    {
        return _commands;
    }

    public override ICommandItem[] GetDockBands()
    {
        return [new CodexBarDockBand(_state)];
    }
}
