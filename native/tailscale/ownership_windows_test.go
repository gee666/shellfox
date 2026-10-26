package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestOwnershipWindowsPrivateACLDoesNotChangeAppdataRoot(t *testing.T) {
	dir := t.TempDir()
	const information = windows.OWNER_SECURITY_INFORMATION | windows.GROUP_SECURITY_INFORMATION | windows.DACL_SECURITY_INFORMATION
	before, err := windows.GetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT, information)
	if err != nil {
		t.Fatal(err)
	}
	file, locked, err := acquireOwnership(dir)
	if err != nil || !locked {
		t.Fatalf("ownership: %v", err)
	}
	defer file.Close()
	after, err := windows.GetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT, information)
	if err != nil || before.String() != after.String() {
		t.Fatal("ownership changed appdata root security")
	}
	security, err := ownershipSecurity(false)
	if err != nil {
		t.Fatal(err)
	}
	want := strings.Split(security.SecurityDescriptor.String(), ";")
	for _, path := range []string{filepath.Join(dir, ownershipDirectory), file.Name()} {
		sd, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION)
		if err != nil {
			t.Fatal(err)
		}
		control, _, err := sd.Control()
		if err != nil || control&windows.SE_DACL_PROTECTED == 0 {
			t.Fatal("ownership inherits unrelated permissions")
		}
		dacl, _, err := sd.DACL()
		if err != nil || dacl == nil || dacl.AceCount != 1 {
			t.Fatal("ownership has extra ACL entries")
		}
		ace := strings.Split(sd.String(), ";")
		if len(ace) != 6 || ace[2] != "FA" || ace[5] != want[5] {
			t.Fatal("ownership ACL is not current-user only")
		}
	}
	if err := os.Remove(file.Name()); err == nil {
		t.Fatal("Windows lock path could be deleted while held")
	}
}
