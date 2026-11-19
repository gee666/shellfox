using System.ComponentModel;
using Shellfox.ElevatedTerminal;

if (!OperatingSystem.IsWindowsVersionAtLeast(10, 0, 17763))
{
    if (args.Length > 0 && args[0] == "pipe-run") Console.Error.WriteLine("Shellfox pipe-run requires Windows 10 version 1809 or newer.");
    return 2;
}
try
{
    if (args is ["self-test"]) return await SelfTest.Run();
    if (args is ["self-test", "--profiler-dll", var dll] && Path.IsPathFullyQualified(dll) && File.Exists(dll)) return await SelfTest.Run(dll);
    if (args is ["probe"]) { Console.WriteLine("shellfox-elevated-conpty-v1"); return 0; }
    if (args is ["probe-isolated"]) { Console.WriteLine(IsolatedLaunch.Probe); return 0; }
    if (args is ["probe-power"]) { Console.WriteLine(PowerProtocol.Probe); return 0; }
    if (args is ["power-guardian"]) return PowerGuardian.Run();
    if (args is ["pipe-run", var config]) return PipeRun.Run(config);
    if (args.Length > 0 && args[0] == "pipe-run") throw new InvalidDataException("Expected one config path.");
    if (args is ["launch"]) return await Launcher.Run();
    if (args is ["launch-isolated"]) return await Launcher.Run(isolated: true);
    if (args is ["serve" or "serve-isolated", var name, var pid, var birth] && name.StartsWith("shellfox-admin-", StringComparison.Ordinal) && name.Length == 47 &&
        Guid.TryParseExact(name[15..], "N", out _) && int.TryParse(pid, out var parentPid) && parentPid > 0 && long.TryParse(birth, out var parentBirth))
        return await Helper.Run(name, parentPid, parentBirth, isolated: args[0] == "serve-isolated");
    return 2;
}
catch (Exception error)
{
    if (args.Length > 0 && args[0] == "self-test") Console.Error.WriteLine(error);
    if (args.Length > 0 && args[0] == "pipe-run") Console.Error.WriteLine(error is Win32Exception win ? $"Shellfox pipe-run failed, Windows error {win.NativeErrorCode}." : "Shellfox pipe-run failed.");
    return 1;
}
