using System.Reflection;
using System.Runtime.InteropServices;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class LoaderPolicyTests
{
    private static readonly string[] Loaders = ["LoadLibraryW", "LoadLibraryExW", "GetProcAddress", "LdrLoadDll", "SetDllDirectoryW", "AddDllDirectory"];

    [Fact]
    public void EveryApplicationImportIsAFixedSystem32OsLibrary()
    {
        var types = new[] { typeof(ConPty), typeof(PseudoConsoles), typeof(LocalPipe), typeof(PipeRun) };
        var libraries = new[] { "kernel32.dll", "advapi32.dll" };
        var count = 0;
        var loaders = new List<string>();
        foreach (var type in types)
        foreach (var method in type.GetMethods(BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public))
        {
            var import = method.GetCustomAttribute<DllImportAttribute>();
            if (import == null) continue;
            count++;
            Assert.Contains(import.Value, libraries);
            Assert.Equal(DllImportSearchPath.System32, method.GetCustomAttribute<DefaultDllImportSearchPathsAttribute>()?.Paths);
            if (Loaders.Contains(import.EntryPoint ?? method.Name)) loaders.Add($"{type.Name}.{import.EntryPoint ?? method.Name}");
        }
        Assert.Equal(41, count);
        // The one dynamic load: the verified ConPTY copy, see PseudoConsoles.
        Assert.Equal(["PseudoConsoles.GetProcAddress", "PseudoConsoles.LoadLibraryExW"], loaders.Order());
    }
}
