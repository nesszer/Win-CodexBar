using CodexBar.CommandPalette.Models;

namespace CodexBar.CommandPalette.Ui;

internal static class ProviderText
{
    public static string Title(ProviderSnapshot provider)
    {
        return provider.HasError
            ? $"{provider.DisplayName} error"
            : $"{provider.DisplayName} {provider.UsedPercent:0}%";
    }

    public static string FiveHourTitle(ProviderSnapshot provider)
    {
        if (provider.HasError)
        {
            return $"{provider.DisplayName} error";
        }

        return provider.Primary is null
            ? $"{provider.DisplayName} --"
            : $"{provider.DisplayName} 5h {provider.Primary.UsedPercent:0}%";
    }

    public static string FiveHourSubtitle(ProviderSnapshot provider)
    {
        if (!string.IsNullOrWhiteSpace(provider.Error))
        {
            return Shorten(provider.Error, 90);
        }

        var reset = provider.Primary?.ResetDescription;
        if (!string.IsNullOrWhiteSpace(reset))
        {
            return $"5h reset {reset}";
        }

        return string.IsNullOrWhiteSpace(provider.Source)
            ? "No 5h reset status"
            : provider.Source;
    }

    public static string Subtitle(ProviderSnapshot provider)
    {
        if (!string.IsNullOrWhiteSpace(provider.Error))
        {
            return Shorten(provider.Error, 90);
        }

        var reset = provider.Primary?.ResetDescription
            ?? provider.Secondary?.ResetDescription
            ?? provider.Source;

        return string.IsNullOrWhiteSpace(reset)
            ? "No reset status"
            : reset;
    }

    public static string WindowLine(string label, RateWindowSnapshot? window)
    {
        if (window is null)
        {
            return $"{label}: unavailable";
        }

        var reset = string.IsNullOrWhiteSpace(window.ResetDescription)
            ? "reset unknown"
            : $"reset {window.ResetDescription}";
        return $"{label}: {window.UsedPercent:0}% used, {window.RemainingPercent:0}% remaining, {reset}";
    }

    private static string Shorten(string value, int maxLength)
    {
        return value.Length <= maxLength ? value : value[..(maxLength - 1)] + "...";
    }
}
