using System.ComponentModel;
using System.Diagnostics;
using System.Text.Json.Nodes;

namespace Shellfox.ElevatedTerminal;

// The serve process can't report over the pipe until it authenticates the
// launcher. Its original ShellExecute handle supplies the startup exit code.
internal enum StartupStage
{
    LaunchRequest = 10,
    Consent,
    Connect,
    VerifyHelper,
    SendLaunch,
    Ready,
    Elevation,
    OpenLauncher,
    ConnectToLauncher,
    VerifyLauncher,
    ReadLaunch,
    CreatePowerShell,
    SendReady,
    Active,
}

internal static class StartupFailure
{
    internal static JsonObject Frame(StartupStage stage, Exception error, bool cancelled)
    {
        var denied = stage == StartupStage.Consent && error is Win32Exception { NativeErrorCode: 1223 };
        var (code, message) = denied ? ("cancelled", "Windows administrator approval was denied or dismissed.")
            : stage switch
            {
                StartupStage.LaunchRequest => ("failed", "Administrator PowerShell failed at reading the launch request."),
                StartupStage.Consent when cancelled => ("cancelled", "Administrator PowerShell was cancelled while waiting for Windows approval."),
                StartupStage.Consent => ("failed", "Windows could not start the elevated terminal helper."),
                StartupStage.Connect => Started("connecting to the elevated helper"),
                StartupStage.VerifyHelper => Started("authenticating the elevated helper"),
                StartupStage.SendLaunch => Started("sending the launch request"),
                StartupStage.Ready => Started("waiting for READY"),
                StartupStage.Elevation => Started("checking elevation"),
                StartupStage.OpenLauncher => Started("opening the launcher process"),
                StartupStage.ConnectToLauncher => Started("connecting to the launcher pipe"),
                StartupStage.VerifyLauncher => Started("authenticating the launcher"),
                StartupStage.ReadLaunch => Started("reading the launch request in the helper"),
                StartupStage.CreatePowerShell => Started("creating the PowerShell ConPTY"),
                StartupStage.SendReady => Started("sending READY"),
                _ when cancelled => ("cancelled", "Administrator PowerShell was cancelled or timed out."),
                _ => ("failed", "The elevated terminal helper could not start or disconnected."),
            };
        return Wire.Object(("type", "error"), ("code", code), ("message", message));
    }

    private static (string, string) Started(string stage) => ("failed", $"Elevated helper started but failed at {stage}.");

    internal static async Task<StartupStage> ReadPeerStage(Process child, StartupStage fallback)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(1));
        try
        {
            await child.WaitForExitAsync(timeout.Token);
            var stage = (StartupStage)child.ExitCode;
            if (stage is >= StartupStage.Elevation and <= StartupStage.SendReady) return stage;
        }
        catch (OperationCanceledException) { }
        catch (InvalidOperationException) { }
        catch (Win32Exception) { }
        return fallback;
    }
}
