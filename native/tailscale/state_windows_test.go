package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestWindowsStateDACLIsUserOnlyWithoutChangingOwnerOrGroup(t *testing.T) {
	dir := filepath.Join(t.TempDir(), strings.Repeat("long-state-path-", 3), "tailscale")
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	const information = windows.OWNER_SECURITY_INFORMATION | windows.GROUP_SECURITY_INFORMATION | windows.DACL_SECURITY_INFORMATION
	before, err := windows.GetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT, information)
	if err != nil {
		t.Fatal(err)
	}
	ownerBefore, _, _ := before.Owner()
	groupBefore, _, _ := before.Group()
	if err := prepareState(dir); err != nil {
		t.Fatal(err)
	}
	after, err := windows.GetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT, information)
	if err != nil {
		t.Fatal(err)
	}
	ownerAfter, _, _ := after.Owner()
	groupAfter, _, _ := after.Group()
	if !ownerBefore.Equals(ownerAfter) || !groupBefore.Equals(groupAfter) {
		t.Fatal("state protection changed owner or primary group")
	}
	control, _, err := after.Control()
	if err != nil || control&windows.SE_DACL_PROTECTED == 0 {
		t.Fatal("state directory inherits unrelated parent permissions")
	}
	store, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	if err := store.WriteState("identity", []byte("durable")); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{dir, store.path} {
		sd, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION)
		if err != nil {
			t.Fatal(err)
		}
		dacl, _, err := sd.DACL()
		if err != nil || dacl == nil || dacl.AceCount != 1 {
			t.Fatal("state has extra ACL entries")
		}
		user, err := windows.GetCurrentProcessToken().GetTokenUser()
		if err != nil {
			t.Fatal(err)
		}
		expected, err := windows.SecurityDescriptorFromString("D:P(A;;FA;;;" + user.User.Sid.String() + ")")
		if err != nil {
			t.Fatal(err)
		}
		// Compare the canonical trustee and rights, ignoring directory inheritance flags.
		ace := strings.Split(sd.String(), ";")
		want := strings.Split(expected.String(), ";")
		if len(ace) != 6 || len(want) != 6 || ace[2] != "FA" || ace[5] != want[5] {
			t.Fatal("state ACL does not grant only the current user full control")
		}
	}
}
