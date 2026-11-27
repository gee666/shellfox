using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

// The inbox kernel32 ConPTY drops mouse-mode changes, so full-screen TUIs such
// as pi lose the wheel. Microsoft's conpty.dll (vendor/conpty) passes them. It
// ships in the install's bin/, which the user can write, so the elevated
// helper never loads it from there. It copies the files, after checking them
// against the hashes below, into a directory only trusted principals can
// write, and loads them from that copy. conpty.dll starts the OpenConsole.exe
// next to itself, so both live there. Anything unexpected means kernel32.
internal sealed unsafe class PseudoConsoles
{
    internal const string Version = "1.25.261109001";
    internal const string Library = "conpty.dll", Host = "OpenConsole.exe";
    private const int MaxImageBytes = 8 * 1024 * 1024;
    // LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
    private const uint LoadFlags = 0x00000100 | 0x00000800;
    // Rights that can change an object's content, name or security.
    private const FileSystemRights WriteRights = FileSystemRights.WriteData | FileSystemRights.AppendData |
        FileSystemRights.WriteExtendedAttributes | FileSystemRights.WriteAttributes | FileSystemRights.DeleteSubdirectoriesAndFiles |
        FileSystemRights.Delete | FileSystemRights.ChangePermissions | FileSystemRights.TakeOwnership |
        (FileSystemRights)0x50000000; // GENERIC_WRITE | GENERIC_ALL

    private readonly delegate* unmanaged[Stdcall]<Coord, IntPtr, IntPtr, uint, IntPtr*, int> create;
    private readonly delegate* unmanaged[Stdcall]<IntPtr, Coord, int> resize;
    private readonly delegate* unmanaged[Stdcall]<IntPtr, void> close;

    internal static PseudoConsoles Inbox { get; } = new();
    internal bool Bundled => create != null;

    private PseudoConsoles() { }
    private PseudoConsoles(IntPtr library)
    {
        create = (delegate* unmanaged[Stdcall]<Coord, IntPtr, IntPtr, uint, IntPtr*, int>)Export(library, "ConptyCreatePseudoConsole");
        resize = (delegate* unmanaged[Stdcall]<IntPtr, Coord, int>)Export(library, "ConptyResizePseudoConsole");
        close = (delegate* unmanaged[Stdcall]<IntPtr, void>)Export(library, "ConptyClosePseudoConsole");
    }

    // SHA-256 of the files in vendor/conpty's package for this architecture.
    internal static (string Library, string Host)? Pins => RuntimeInformation.ProcessArchitecture switch
    {
        Architecture.X64 => ("e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492", "b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775"),
        Architecture.Arm64 => ("3608cef830a1c4fbbf3981594ad3f64ebe2df6db98fe43a48a4ca7d2875d8959", "c04334f450b5731930b63a28ac4da8a058367242dbcf2fb9b323ea25e5a23252"),
        _ => null,
    };

    internal static SecurityIdentifier[] Administrators { get; } =
    [
        new(WellKnownSidType.LocalSystemSid, null),
        new(WellKnownSidType.BuiltinAdministratorsSid, null),
        new("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"), // TrustedInstaller
    ];

    // Packages put the helper in resources/elevated-terminal and ConPTY in bin/.
    // Program Files is writable only by administrators.
    internal static PseudoConsoles ForElevatedHelper() => Load(
        Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "bin")),
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Shellfox"),
        Administrators);

    // source may be any directory: only bytes matching the pins are copied.
    // root and its subdirectory are created writable by `trusted` alone, or
    // must already be so. Tests pass the current user as trusted.
    internal static PseudoConsoles Load(string source, string root, IReadOnlyCollection<SecurityIdentifier> trusted)
    {
        try
        {
            if (Pins is not { } pins) return Inbox;
            var (library, host) = pins;
            var directory = Path.Combine(root, $"ConPTY-{Version}-{RuntimeInformation.ProcessArchitecture.ToString().ToLowerInvariant()}");
            if (!TrustedDirectory(root, trusted) || !TrustedDirectory(directory, trusted)) return Inbox;
            foreach (var (name, pin) in new[] { (Library, library), (Host, host) })
                if (!Install(Path.Combine(source, name), Path.Combine(directory, name), pin, trusted)) return Inbox;
            var module = LoadLibraryExW(Path.Combine(directory, Library), IntPtr.Zero, LoadFlags);
            return module == IntPtr.Zero ? Inbox : new PseudoConsoles(module);
        }
        catch (Exception) { return Inbox; } // Missing files, ACL or I/O errors, a missing export.
    }

    private static bool TrustedDirectory(string path, IReadOnlyCollection<SecurityIdentifier> trusted)
    {
        var info = new DirectoryInfo(path);
        if (!info.Exists)
        {
            var security = new DirectorySecurity();
            security.SetAccessRuleProtection(true, false);
            foreach (var sid in trusted)
                security.AddAccessRule(new FileSystemAccessRule(sid, FileSystemRights.FullControl,
                    InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit, PropagationFlags.None, AccessControlType.Allow));
            info.Create(security);
            info.Refresh();
        }
        return !info.Attributes.HasFlag(FileAttributes.ReparsePoint) && WritableOnlyBy(info.GetAccessControl(), trusted);
    }

    // Owner and every allow entry that applies to the object itself.
    internal static bool WritableOnlyBy(FileSystemSecurity security, IReadOnlyCollection<SecurityIdentifier> trusted)
    {
        if (security.GetOwner(typeof(SecurityIdentifier)) is not SecurityIdentifier owner || !trusted.Contains(owner)) return false;
        foreach (FileSystemAccessRule rule in security.GetAccessRules(true, true, typeof(SecurityIdentifier)))
        {
            if (rule.AccessControlType != AccessControlType.Allow || rule.PropagationFlags.HasFlag(PropagationFlags.InheritOnly)) continue;
            if ((rule.FileSystemRights & WriteRights) != 0 && !trusted.Contains((SecurityIdentifier)rule.IdentityReference)) return false;
        }
        return true;
    }

    // Keeps a matching copy, else writes the verified source bytes and checks again.
    private static bool Install(string source, string target, string pin, IReadOnlyCollection<SecurityIdentifier> trusted)
    {
        if (Matches(target, pin, trusted)) return true;
        var bytes = Read(source);
        if (bytes == null || Convert.ToHexStringLower(SHA256.HashData(bytes)) != pin) return false;
        var temporary = target + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            File.WriteAllBytes(temporary, bytes);
            // Fails while another elevated tab has the old copy loaded; the check below decides.
            try { File.Move(temporary, target, true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        finally { File.Delete(temporary); }
        return Matches(target, pin, trusted);
    }

    private static bool Matches(string path, string pin, IReadOnlyCollection<SecurityIdentifier> trusted)
    {
        var info = new FileInfo(path);
        if (!info.Exists || info.Attributes.HasFlag(FileAttributes.ReparsePoint) || !WritableOnlyBy(info.GetAccessControl(), trusted)) return false;
        var bytes = Read(path);
        return bytes != null && Convert.ToHexStringLower(SHA256.HashData(bytes)) == pin;
    }

    private static byte[]? Read(string path)
    {
        if (!File.Exists(path)) return null;
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
        if (stream.Length is 0 or > MaxImageBytes) return null;
        var bytes = new byte[stream.Length];
        stream.ReadExactly(bytes);
        return stream.ReadByte() == -1 ? bytes : null;
    }

    private static IntPtr Export(IntPtr library, string name)
    {
        var address = GetProcAddress(library, name);
        return address != IntPtr.Zero ? address : throw new EntryPointNotFoundException(name);
    }

    internal int Create(Coord size, SafeFileHandle input, SafeFileHandle output, out IntPtr console)
    {
        if (!Bundled) return CreatePseudoConsole(size, input, output, 0, out console);
        IntPtr created = IntPtr.Zero;
        var result = create(size, input.DangerousGetHandle(), output.DangerousGetHandle(), 0, &created);
        GC.KeepAlive(input); GC.KeepAlive(output);
        console = created;
        return result;
    }
    internal int Resize(IntPtr console, Coord size) => Bundled ? resize(console, size) : ResizePseudoConsole(console, size);
    internal void Close(IntPtr console) { if (Bundled) close(console); else ClosePseudoConsole(console); }

    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", CharSet = CharSet.Ansi, ExactSpelling = true)] private static extern IntPtr GetProcAddress(IntPtr module, string name);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern int CreatePseudoConsole(Coord size, SafeFileHandle input, SafeFileHandle output, uint flags, out IntPtr console);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern int ResizePseudoConsole(IntPtr console, Coord size);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern void ClosePseudoConsole(IntPtr console);
}

[StructLayout(LayoutKind.Sequential)] internal struct Coord { public short X, Y; }
