using CodexBar.CommandPalette.Models;

namespace CodexBar.CommandPalette.Services;

public interface ICodexBarCliClient
{
    Task<CmdPalSnapshot> GetSnapshotAsync(CancellationToken cancellationToken);
}
