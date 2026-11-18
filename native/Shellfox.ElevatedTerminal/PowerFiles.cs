using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

// Owns pinned directory handles. No ancestor can be replaced while the journal
// lease is held. This is not a sandbox against an administrator or same-user code.
internal sealed class PowerFiles : IDisposable
{
    private readonly List<SafeFileHandle> directories = new();
    private readonly string root;
    private FileStream? lease;
    internal PowerFiles(string path)
    {
        root = LocalPath(path);
        if (root == Path.GetPathRoot(root) || new DriveInfo(Path.GetPathRoot(root)!).DriveType != DriveType.Fixed)
            throw new InvalidDataException("Private fixed-volume power root required.");
        try
        {
            var ancestors = new Stack<string>();
            for (string? current = root; current != null; current = Path.GetDirectoryName(current)) ancestors.Push(current);
            foreach (var directory in ancestors)
            {
                var handle = CreateFile(directory, 0x20080, 3, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero);
                if (handle.IsInvalid) { handle.Dispose(); throw new Win32Exception(Marshal.GetLastWin32Error()); }
                directories.Add(handle);
                var info = Information(handle);
                if ((info.Attributes & 0x410) != 0x10) throw new InvalidDataException("Power root is redirected.");
            }
            ValidateAcl(new DirectoryInfo(root).GetAccessControl(), requireProtected: true);
            lease = Open("guardian.lock");
        }
        catch { Dispose(); throw; }
    }
    internal static string LocalPath(string? value)
    {
        if (value is null || value.Length is < 3 or > 32700 || !char.IsAsciiLetter(value[0]) || value[1] != ':' || value[2] is not ('\\' or '/') ||
            value.Skip(2).Any(c => c < 32 || c is ':' or '"' or '*' or '?' or '<' or '>' or '|'))
            throw new InvalidDataException("Expected an absolute local power root.");
        foreach (var part in value[3..].Split(new[] { '/', '\\' }, StringSplitOptions.RemoveEmptyEntries))
            if (part is "." or ".." || part.EndsWith('.') || part.EndsWith(' ')) throw new InvalidDataException("Ambiguous power root.");
        return Path.TrimEndingDirectorySeparator(Path.GetFullPath(value));
    }
    internal FileStream Open(string name)
    {
        if (name is not ("guardian.lock" or "rollback.log")) throw new InvalidDataException("Invalid power file.");
        var path = Path.Combine(root, name);
        // OPEN_ALWAYS, no sharing, OPEN_REPARSE_POINT, WRITE_THROUGH. A reparse
        // leaf is opened as the link itself and rejected before any file write.
        using var identity = WindowsIdentity.GetCurrent();
        var sid = identity.User?.Value ?? throw new UnauthorizedAccessException("No power owner SID.");
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor($"O:{sid}D:P(A;;FA;;;{sid})(A;;FA;;;SY)", 1, out var descriptor, out _))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        SafeFileHandle handle;
        try
        {
            var security = new SecurityAttributes { Size = Marshal.SizeOf<SecurityAttributes>(), Descriptor = descriptor };
            handle = CreatePrivateFile(path, 0xC0030000, 0, ref security, 4, 0x80200000, IntPtr.Zero);
        }
        finally { LocalFree(descriptor); }
        if (handle.IsInvalid) { handle.Dispose(); throw new Win32Exception(Marshal.GetLastWin32Error()); }
        try
        {
            var info = Information(handle);
            if ((info.Attributes & 0x410) != 0 || info.Links != 1) throw new InvalidDataException("Power file is redirected or shared.");
            // The file is exclusively open and its ancestors are pinned. Query
            // security through its handle, not a second name-based open.
            ValidateAcl(ReadSecurity(handle), requireProtected: false);
            return new FileStream(handle, FileAccess.ReadWrite, 4096, false);
        }
        catch { handle.Dispose(); throw; }
    }
    private static FileSecurity ReadSecurity(SafeFileHandle handle)
    {
        GetKernelObjectSecurity(handle, 7, null, 0, out var size);
        if (size is < 1 or > 65536) throw new InvalidDataException("Invalid power file security.");
        var bytes = new byte[size];
        if (!GetKernelObjectSecurity(handle, 7, bytes, size, out _)) throw new Win32Exception(Marshal.GetLastWin32Error());
        var result = new FileSecurity(); result.SetSecurityDescriptorBinaryForm(bytes); return result;
    }
    private static void ValidateAcl(FileSystemSecurity security, bool requireProtected)
    {
        using var identity = WindowsIdentity.GetCurrent();
        var user = identity.User ?? throw new UnauthorizedAccessException("No power owner SID.");
        if (!user.Equals(security.GetOwner(typeof(SecurityIdentifier))) || requireProtected && !security.AreAccessRulesProtected)
            throw new UnauthorizedAccessException("Power state must be privately owned.");
        var allowed = false;
        foreach (FileSystemAccessRule rule in security.GetAccessRules(true, true, typeof(SecurityIdentifier)))
        {
            if (rule.AccessControlType != AccessControlType.Allow) continue;
            if (!rule.IdentityReference.Equals(user) && !rule.IdentityReference.Equals(new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null)))
                throw new UnauthorizedAccessException("Power state has nonprivate access.");
            if (rule.IdentityReference.Equals(user) && (rule.FileSystemRights & FileSystemRights.FullControl) == FileSystemRights.FullControl) allowed = true;
        }
        if (!allowed) throw new UnauthorizedAccessException("Power owner requires full state access.");
    }
    internal static void DeleteOnClose(FileStream file)
    {
        var value = 1;
        if (!SetFileInformationByHandle(file.SafeFileHandle, 4, ref value, 4)) throw new Win32Exception(Marshal.GetLastWin32Error());
    }
    private static FileInfo Information(SafeFileHandle handle)
    {
        if (!GetFileInformationByHandle(handle, out var info)) throw new Win32Exception(Marshal.GetLastWin32Error());
        return info;
    }
    public void Dispose()
    {
        lease?.Dispose(); lease = null;
        foreach (var handle in directories) handle.Dispose();
        directories.Clear();
    }
    [StructLayout(LayoutKind.Sequential)] private struct FileInfo
    {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [StructLayout(LayoutKind.Sequential)] private struct SecurityAttributes { public int Size; public IntPtr Descriptor; public int Inherit; }
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(string sddl, uint revision, out IntPtr descriptor, out uint size);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr value);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreatePrivateFile(string name, uint access, uint share, ref SecurityAttributes security, uint disposition, uint flags, IntPtr template);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInfo info);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("advapi32.dll", SetLastError = true)] private static extern bool GetKernelObjectSecurity(SafeFileHandle handle, uint information, byte[]? bytes, uint size, out uint needed);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetFileInformationByHandle(SafeFileHandle handle, int type, ref int info, int size);
}
