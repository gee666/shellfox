using System.Reflection;
using System.Runtime.InteropServices;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class LoaderPolicyTests
{
    [Fact]
    public void EveryApplicationImportIsAFixedSystem32OsLibrary()
    {
        var types = new[] { typeof(ConPty), typeof(LocalPipe), typeof(PipeRun) };
        var libraries = new[] { "kernel32.dll", "advapi32.dll" };
        var count = 0;
        foreach (var type in types)
        foreach (var method in type.GetMethods(BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public))
        {
            var import = method.GetCustomAttribute<DllImportAttribute>();
            if (import == null) continue;
            count++;
            Assert.Contains(import.Value, libraries);
            Assert.Equal(DllImportSearchPath.System32, method.GetCustomAttribute<DefaultDllImportSearchPathsAttribute>()?.Paths);
            Assert.DoesNotContain(import.EntryPoint!, new[] { "LoadLibraryW", "LoadLibraryExW", "GetProcAddress", "LdrLoadDll", "SetDllDirectoryW", "AddDllDirectory" });
        }
        Assert.Equal(39, count);
    }
}
