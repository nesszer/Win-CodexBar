using System.Runtime.InteropServices;
using Microsoft.CommandPalette.Extensions;

namespace CodexBar.CommandPalette;

[ComVisible(true)]
[Guid("C6A683A4-6101-4F13-83FE-EE355BA58FE3")]
[ComDefaultInterface(typeof(IExtension))]
public sealed partial class CodexBarExtension : IExtension, IDisposable
{
    private readonly ManualResetEvent _extensionDisposedEvent;
    private readonly CodexBarCommandsProvider _provider = new();

    public CodexBarExtension(ManualResetEvent extensionDisposedEvent)
    {
        _extensionDisposedEvent = extensionDisposedEvent;
    }

    public object? GetProvider(ProviderType providerType)
    {
        return providerType == ProviderType.Commands ? _provider : null;
    }

    public void Dispose()
    {
        _extensionDisposedEvent.Set();
    }
}
