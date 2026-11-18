using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

// Filesystem-only fixtures. No PowerNative, PowerOwner or real guardian run.
public sealed class PowerJournalTests : IDisposable
{
    private readonly string root;
    public PowerJournalTests()
    {
        var project = new DirectoryInfo(Environment.CurrentDirectory);
        while (project != null && !File.Exists(Path.Combine(project.FullName, "native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj"))) project = project.Parent;
        if (project == null) throw new InvalidOperationException("Run inside the worktree.");
        root = Path.Combine(project.FullName, "tmp", "power-journal-tests-space & ' " + Guid.NewGuid().ToString("N"));
        new DirectoryInfo(root).Create(PrivateAcl());
    }
    private static DirectorySecurity PrivateAcl(bool shared = false)
    {
        using var identity = WindowsIdentity.GetCurrent(); var user = identity.User!;
        var acl = new DirectorySecurity(); acl.SetOwner(user); acl.SetAccessRuleProtection(true, false);
        var inheritance = InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit;
        acl.AddAccessRule(new(user, FileSystemRights.FullControl, inheritance, PropagationFlags.None, AccessControlType.Allow));
        acl.AddAccessRule(new(new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null), FileSystemRights.FullControl, inheritance, PropagationFlags.None, AccessControlType.Allow));
        if (shared) acl.AddAccessRule(new(new SecurityIdentifier(WellKnownSidType.WorldSid, null), FileSystemRights.Read, inheritance, PropagationFlags.None, AccessControlType.Allow));
        return acl;
    }
    private static PowerSnapshot Snapshot() => new(Guid.NewGuid(), 1, 2, true, true, true);
    [Fact]
    public void DurableSnapshotSurvivesHardCrashAndCompletedRecoveryDeletesJournal()
    {
        var state = Snapshot();
        using (var files = new PowerFiles(root))
        using (var journal = new PowerJournal(files)) { journal.Save(state); }
        using (var files = new PowerFiles(root))
        using (var journal = new PowerJournal(files))
        {
            Assert.Equal(state, journal.Current);
            journal.Clear(); Assert.Null(journal.Current);
        }
        Assert.False(File.Exists(Path.Combine(root, "rollback.log")));
    }
    [Fact]
    public void TornTailUsesPreviousFlushedIntentAndNextSaveRemovesOnlyTail()
    {
        var state = Snapshot(); var path = Path.Combine(root, "rollback.log");
        using var files = new PowerFiles(root);
        using (var journal = new PowerJournal(files)) journal.Save(state);
        var length = new FileInfo(path).Length;
        using (var stream = new FileStream(path, FileMode.Append, FileAccess.Write)) stream.Write(new byte[] { 100, 0, 0, 0, 123 });
        using (var journal = new PowerJournal(files))
        {
            Assert.Equal(state, journal.Current);
            journal.Save(state with { AcOwned = false });
        }
        using var recovered = new PowerJournal(files);
        Assert.Equal(state with { AcOwned = false }, recovered.Current);
        Assert.True(new FileInfo(path).Length > length);
    }
    [Fact]
    public void CorruptFirstRecordIsRetainedAndNeverGuessedOrErased()
    {
        var path = Path.Combine(root, "rollback.log");
        using var files = new PowerFiles(root);
        using (var journal = new PowerJournal(files)) journal.Save(Snapshot());
        var bytes = File.ReadAllBytes(path); bytes[^1] ^= 1; File.WriteAllBytes(path, bytes);
        Assert.Throws<InvalidDataException>(() => new PowerJournal(files));
        Assert.Equal(bytes, File.ReadAllBytes(path));
    }
    [Fact]
    public void ExclusiveLeaseRejectsConcurrentGuardiansAndPinsRootAgainstRename()
    {
        using var files = new PowerFiles(root);
        Assert.ThrowsAny<Exception>(() => new PowerFiles(root));
        Assert.Throws<IOException>(() => Directory.Move(root, root + "-moved"));
    }
    [Fact]
    public void InheritedOrBroadRootAclIsRejected()
    {
        var inherited = Path.Combine(root, "inherited"); Directory.CreateDirectory(inherited);
        Assert.Throws<UnauthorizedAccessException>(() => new PowerFiles(inherited));
        var shared = Path.Combine(root, "shared"); new DirectoryInfo(shared).Create(PrivateAcl(shared: true));
        Assert.Throws<UnauthorizedAccessException>(() => new PowerFiles(shared));
    }
    [Fact]
    public void JournalAndLeaseHardlinksAreRejectedWithoutTouchingOutsideFixture()
    {
        var source = Path.Combine(root, "owned-source"); File.WriteAllText(source, "owned original");
        var log = Path.Combine(root, "rollback.log");
        Assert.True(CreateHardLink(log, source, IntPtr.Zero));
        using (var files = new PowerFiles(root)) Assert.Throws<InvalidDataException>(() => new PowerJournal(files));
        File.Delete(log);
        Assert.False(CreateHardLink(Path.Combine(root, "guardian.lock"), source, IntPtr.Zero)); // Existing lock cannot be replaced.
        File.Delete(Path.Combine(root, "guardian.lock"));
        Assert.True(CreateHardLink(Path.Combine(root, "guardian.lock"), source, IntPtr.Zero));
        Assert.Throws<InvalidDataException>(() => new PowerFiles(root));
        Assert.Equal("owned original", File.ReadAllText(source));
    }
    [Fact]
    public void DirectoryAndAncestorRedirectsAreRejected()
    {
        var target = Path.Combine(root, "target"); new DirectoryInfo(target).Create(PrivateAcl());
        var link = Path.Combine(root, "redirect"); Junction.Create(link, target);
        try { Assert.Throws<InvalidDataException>(() => new PowerFiles(link)); }
        finally { Directory.Delete(link); }
        Directory.CreateDirectory(Path.Combine(root, "rollback.log"));
        using var files = new PowerFiles(root);
        Assert.ThrowsAny<Exception>(() => new PowerJournal(files));
    }
    [Fact]
    public void OversizedJournalFailsClosedAndIsRetained()
    {
        var path = Path.Combine(root, "rollback.log");
        using var files = new PowerFiles(root);
        using (var journal = new PowerJournal(files)) journal.Save(Snapshot());
        using (var stream = File.OpenWrite(path)) stream.SetLength(1024 * 1024 + 1);
        Assert.Throws<InvalidDataException>(() => new PowerJournal(files));
        Assert.Equal(1024 * 1024 + 1, new FileInfo(path).Length);
    }
    [Fact]
    public void NextStartupRestoresDurablePartialMutationUsingFakeOsOnly()
    {
        var native = new PowerFakeNative();
        using (var files = new PowerFiles(root))
        using (var journal = new PowerJournal(files))
        {
            native.Journal = journal;
            native.Fault = op => { if (op == "write-dc-0") throw new IOException("simulated process death"); };
            Assert.Throws<IOException>(() => new PowerLease(native, journal).Enable());
            Assert.Equal(0u, native.Values[(native.Scheme, true)]);
        }
        native.HeldRequests = 0; // Kernel closes request handles on process death.
        native.Fault = null;
        using (var files = new PowerFiles(root))
        using (var journal = new PowerJournal(files))
        {
            native.Journal = journal;
            var controller = new PowerController(new(native, journal)); controller.Go("recover", 0);
            Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.Null(journal.Current);
        }
        Assert.Equal(1u, native.Values[(native.Scheme, true)]); Assert.Equal(2u, native.Values[(native.Scheme, false)]);
        Assert.Equal(0, native.Pulses); Assert.Equal(0, native.HeldRequests);
    }
    [Fact]
    public void JournalSchemeAndOriginalValuesCannotChangeMidLease()
    {
        using var files = new PowerFiles(root); using var journal = new PowerJournal(files);
        var state = Snapshot(); journal.Save(state);
        Assert.Throws<InvalidDataException>(() => journal.Save(state with { Scheme = Guid.NewGuid() }));
        Assert.Throws<InvalidDataException>(() => journal.Save(state with { Ac = 3 }));
        Assert.Equal(state, journal.Current);
    }
    public void Dispose() => Directory.Delete(root, recursive: true);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateHardLink(string name, string existing, IntPtr security);
}
