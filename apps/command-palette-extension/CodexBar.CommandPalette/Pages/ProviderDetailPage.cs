using CodexBar.CommandPalette.Models;
using CodexBar.CommandPalette.Services;
using CodexBar.CommandPalette.Ui;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace CodexBar.CommandPalette.Pages;

internal sealed partial class ProviderDetailPage : ListPage
{
    private readonly CodexBarStateService _state;
    private readonly string _providerId;

    public ProviderDetailPage(CodexBarStateService state, string providerId)
    {
        _state = state;
        _providerId = providerId;
        Name = "Provider";
        Icon = new IconInfo("\uE9D9");
    }

    public override IListItem[] GetItems()
    {
        var provider = _state.FindProvider(_providerId)
            ?? _state.RefreshBlocking().Providers.FirstOrDefault(item =>
                string.Equals(item.ProviderId, _providerId, StringComparison.OrdinalIgnoreCase));

        if (provider is null)
        {
            return
            [
                new ListItem(new NoOpCommand())
                {
                    Title = "Provider unavailable",
                    Subtitle = _providerId,
                    Icon = new IconInfo("\uE783"),
                },
            ];
        }

        Title = provider.DisplayName;
        var items = new List<IListItem>
        {
            InfoItem(provider.DisplayName, ProviderText.Title(provider)),
            InfoItem(provider.PrimaryLabel ?? "Session", ProviderText.WindowLine(provider.PrimaryLabel ?? "Session", provider.Primary)),
            InfoItem(provider.SecondaryLabel ?? "Weekly", ProviderText.WindowLine(provider.SecondaryLabel ?? "Weekly", provider.Secondary)),
            InfoItem("Source", provider.Source ?? "Unavailable"),
            InfoItem("Updated", provider.UpdatedAt?.ToString("yyyy-MM-dd HH:mm:ss zzz") ?? "Unknown"),
        };

        if (!string.IsNullOrWhiteSpace(provider.Error))
        {
            items.Add(InfoItem("Error", provider.Error, "\uE783"));
        }

        AddLinkItems(provider, items);

        return items.ToArray();
    }

    private static ListItem InfoItem(string title, string subtitle, string icon = "\uE946")
    {
        return new ListItem(new NoOpCommand())
        {
            Title = title,
            Subtitle = subtitle,
            Icon = new IconInfo(icon),
        };
    }

    private static void AddLinkItems(ProviderSnapshot provider, List<IListItem> items)
    {
        if (!string.IsNullOrWhiteSpace(provider.DashboardUrl))
        {
            items.Add(new ListItem(new OpenUrlCommand(provider.DashboardUrl))
            {
                Title = "Open Dashboard",
                Subtitle = provider.DashboardUrl,
                Icon = new IconInfo("\uE774"),
            });
        }

        if (!string.IsNullOrWhiteSpace(provider.StatusPageUrl))
        {
            items.Add(new ListItem(new OpenUrlCommand(provider.StatusPageUrl))
            {
                Title = "Open Status Page",
                Subtitle = provider.StatusPageUrl,
                Icon = new IconInfo("\uE946"),
            });
        }
    }
}
