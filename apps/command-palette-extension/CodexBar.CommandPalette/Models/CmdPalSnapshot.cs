using System.Text.Json.Serialization;

namespace CodexBar.CommandPalette.Models;

public sealed record CmdPalSnapshot
{
    public string ContractVersion { get; init; } = string.Empty;

    public DateTimeOffset GeneratedAt { get; init; }

    public int RefreshIntervalSecs { get; init; }

    public IReadOnlyList<ProviderSnapshot> Providers { get; init; } = [];
}

public sealed record ProviderSnapshot
{
    public string ProviderId { get; init; } = string.Empty;

    public string DisplayName { get; init; } = string.Empty;

    public string? PrimaryLabel { get; init; }

    public RateWindowSnapshot? Primary { get; init; }

    public string? SecondaryLabel { get; init; }

    public RateWindowSnapshot? Secondary { get; init; }

    public string? Source { get; init; }

    public DateTimeOffset? UpdatedAt { get; init; }

    public string? Error { get; init; }

    public string? DashboardUrl { get; init; }

    public string? StatusPageUrl { get; init; }

    [JsonIgnore]
    public double UsedPercent => Math.Max(Primary?.UsedPercent ?? 0, Secondary?.UsedPercent ?? 0);

    [JsonIgnore]
    public bool HasError => !string.IsNullOrWhiteSpace(Error);
}

public sealed record RateWindowSnapshot
{
    public double UsedPercent { get; init; }

    public double RemainingPercent { get; init; }

    public int? WindowMinutes { get; init; }

    public DateTimeOffset? ResetsAt { get; init; }

    public string? ResetDescription { get; init; }

    public bool IsExhausted { get; init; }
}
