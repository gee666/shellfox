package main

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func networkTestLocal(t *testing.T) string {
	t.Helper()
	base, err := filepath.Abs("../../tmp")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(base, 0700); err != nil {
		t.Fatal(err)
	}
	root, err := os.MkdirTemp(base, "network-runtime-test-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.RemoveAll(root); err != nil {
			t.Error(err)
		}
	})
	return root
}

func networkTestRoot(t *testing.T) string {
	t.Helper()
	root, handles, err := prepareNetworkRoot(networkTestLocal(t), "x64", "")
	if err != nil {
		closeNetworkHandles(handles)
		t.Fatal(err)
	}
	t.Cleanup(func() { closeNetworkHandles(handles) })
	return root
}

func networkTestSource(t *testing.T, data string) string {
	t.Helper()
	file := filepath.Join(networkTestLocal(t), "source.exe")
	if err := os.WriteFile(file, []byte(data), 0600); err != nil {
		t.Fatal(err)
	}
	return file
}

func TestNetworkRuntimeStageReusesExactBytesAndReplacesIdleVersion(t *testing.T) {
	root := networkTestRoot(t)
	first := networkTestSource(t, "first helper generation")
	runtime, err := stageNetworkExecutable(first, root)
	if err != nil {
		t.Fatal(err)
	}
	before, err := runtime.Stat()
	if err != nil {
		t.Fatal(err)
	}
	_ = runtime.Close()
	runtime, err = stageNetworkExecutable(first, root)
	if err != nil {
		t.Fatal(err)
	}
	after, err := runtime.Stat()
	_ = runtime.Close()
	if err != nil || !os.SameFile(before, after) || before.ModTime() != after.ModTime() {
		t.Fatal("same-hash deployment rewrote the executable")
	}
	second := networkTestSource(t, "second helper generation")
	runtime, err = stageNetworkExecutable(second, root)
	if err != nil {
		t.Fatal(err)
	}
	_ = runtime.Close()
	data, err := os.ReadFile(filepath.Join(root, networkRuntimeName))
	if err != nil || string(data) != "second helper generation" {
		t.Fatal("idle version was not replaced")
	}
	if err := privatePermissions(filepath.Join(root, networkRuntimeName), false, false); err != nil {
		t.Fatalf("executable ACL: %v", err)
	}
}

func TestNetworkRuntimePinnedGenerationBlocksReplacementWithoutFallback(t *testing.T) {
	root := networkTestRoot(t)
	first := networkTestSource(t, "old")
	second := networkTestSource(t, "new")
	pinned, err := stageNetworkExecutable(first, root)
	if err != nil {
		t.Fatal(err)
	}
	defer pinned.Close()
	identical, err := stageNetworkExecutable(first, root)
	if err != nil {
		t.Fatal("same-hash concurrent launch refused", err)
	}
	_ = identical.Close()
	if file, err := stageNetworkExecutable(second, root); err == nil {
		_ = file.Close()
		t.Fatal("replaced an active executable")
	}
	if data, err := os.ReadFile(pinned.Name()); err != nil || string(data) != "old" {
		t.Fatal("busy failure changed the old executable")
	}
	partials, _ := filepath.Glob(filepath.Join(root, ".network-*.tmp"))
	if len(partials) != 0 {
		t.Fatal("failed deployment left partial files")
	}
	_ = pinned.Close()
	replacement, err := stageNetworkExecutable(second, root)
	if err != nil {
		t.Fatal("retry after release failed", err)
	}
	_ = replacement.Close()
}

func TestNetworkRuntimePrivateTreeLeavesApplicationRootAlone(t *testing.T) {
	local := networkTestLocal(t)
	appRoot := filepath.Join(local, "Shellfox")
	if err := os.Mkdir(appRoot, 0700); err != nil {
		t.Fatal(err)
	}
	const information = windows.OWNER_SECURITY_INFORMATION | windows.GROUP_SECURITY_INFORMATION | windows.DACL_SECURITY_INFORMATION
	before, err := windows.GetNamedSecurityInfo(appRoot, windows.SE_FILE_OBJECT, information)
	if err != nil {
		t.Fatal(err)
	}
	root, handles, err := prepareNetworkRoot(local, "x64", "")
	defer closeNetworkHandles(handles)
	if err != nil {
		t.Fatal(err)
	}
	after, err := windows.GetNamedSecurityInfo(appRoot, windows.SE_FILE_OBJECT, information)
	if err != nil || before.String() != after.String() {
		t.Fatal("deployment changed the installation root's ACL")
	}
	for _, path := range []string{filepath.Dir(root), root} {
		if err := privatePermissions(path, true, false); err != nil {
			t.Fatal("unsafe runtime directory", err)
		}
	}
	from, _ := windows.UTF16PtrFromString(root)
	to, _ := windows.UTF16PtrFromString(root + "-moved")
	if err := windows.MoveFileEx(from, to, 0); err == nil {
		t.Fatal("deployment root was not pinned")
	}
}

func TestNetworkRuntimeRejectsUnsafePathsAndLinkedExecutable(t *testing.T) {
	for _, local := range []string{"", ".", `C:relative`, `\\server\share\data`, `\\?\C:\data`} {
		_, handles, err := prepareNetworkRoot(local, "x64", "")
		closeNetworkHandles(handles)
		if err == nil {
			t.Fatalf("accepted unsafe application data %q", local)
		}
	}
	local := networkTestLocal(t)
	_, handles, err := prepareNetworkRoot(local, "../other", "")
	closeNetworkHandles(handles)
	if err == nil {
		t.Fatal("accepted unknown architecture")
	}
	_, handles, err = prepareNetworkRoot(local, "x64", "arbitrary")
	closeNetworkHandles(handles)
	if err == nil {
		t.Fatal("accepted arbitrary runtime channel")
	}
	root := networkTestRoot(t)
	source := networkTestSource(t, "trusted")
	outside := networkTestSource(t, "outside")
	if err := os.Link(outside, filepath.Join(root, networkRuntimeName)); err != nil {
		t.Fatal(err)
	}
	if file, err := stageNetworkExecutable(source, root); err == nil {
		_ = file.Close()
		t.Fatal("accepted hard-linked cached executable")
	}
	data, _ := os.ReadFile(outside)
	if !bytes.Equal(data, []byte("outside")) {
		t.Fatal("modified hard-link target")
	}
}

func TestNetworkRuntimeRejectsDirectorySymlink(t *testing.T) {
	local, outside := networkTestLocal(t), networkTestLocal(t)
	if err := os.Mkdir(filepath.Join(local, "Shellfox"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(local, "Shellfox", "network")); err != nil {
		t.Skipf("Windows symlink privilege unavailable: %v", err)
	}
	_, handles, err := prepareNetworkRoot(local, "x64", "")
	defer closeNetworkHandles(handles)
	if err == nil {
		t.Fatal("accepted linked runtime directory")
	}
}

func TestNetworkRuntimeMissingLocalDataReturnsOnlyFixedFailure(t *testing.T) {
	t.Setenv("LOCALAPPDATA", "")
	for _, args := range [][]string{nil, {"testing"}} {
		var stderr bytes.Buffer
		if code := runNetworkRuntime(args, &stderr); code != 1 || stderr.String() != networkRuntimeFailure+"\n" {
			t.Fatal("runtime failure leaked details or did not fail closed")
		}
	}
}

func TestNetworkRuntimeDeploymentLockHasBoundedContentionAndRetry(t *testing.T) {
	root := networkTestRoot(t)
	lock, err := waitNetworkLock(root, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer lock.Close()
	if second, err := waitNetworkLock(root, 30*time.Millisecond); !errors.Is(err, os.ErrDeadlineExceeded) {
		if second != nil {
			_ = second.Close()
		}
		t.Fatalf("expected bounded contention, got %v", err)
	}
	_ = lock.Close()
	second, err := waitNetworkLock(root, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	_ = second.Close()
}
