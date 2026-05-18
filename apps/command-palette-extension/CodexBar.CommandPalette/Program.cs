using Microsoft.CommandPalette.Extensions;
using Shmuelie.WinRTServer;
using Shmuelie.WinRTServer.CsWinRT;

namespace CodexBar.CommandPalette;

public static class Program
{
    [MTAThread]
    public static void Main(string[] args)
    {
        if (args.Length == 0 || args[0] != "-RegisterProcessAsComServer")
        {
            Console.WriteLine("CodexBar Command Palette extension must be launched by Command Palette.");
            return;
        }

        ManualResetEvent extensionDisposedEvent = new(false);
        var server = new ComServer();
        var extensionInstance = new CodexBarExtension(extensionDisposedEvent);

        server.RegisterClass<CodexBarExtension, IExtension>(() => extensionInstance);
        server.Start();
        extensionDisposedEvent.WaitOne();
        server.Stop();
        server.UnsafeDispose();
        extensionDisposedEvent.Dispose();
    }
}
