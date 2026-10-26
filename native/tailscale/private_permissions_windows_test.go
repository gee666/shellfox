package main

import (
	"bytes"
	"encoding/binary"
	"os"
	"path/filepath"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func currentPrivateUser(t *testing.T) *windows.SID {
	t.Helper()
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		t.Fatal(err)
	}
	return user.User.Sid
}

func privateSD(t *testing.T, sddl string) *windows.SECURITY_DESCRIPTOR {
	t.Helper()
	sd, err := windows.SecurityDescriptorFromString(sddl)
	if err != nil {
		t.Fatal(err)
	}
	return sd
}

func TestWindowsPrivatePermissionsProtectAndVerify(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "private ' Unicode-界")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(dir, "endpoint.json")
	if err := os.WriteFile(file, nil, 0600); err != nil {
		t.Fatal(err)
	}
	for _, target := range []struct {
		path string
		kind string
	}{{dir, "directory"}, {file, "file"}} {
		const info = windows.OWNER_SECURITY_INFORMATION | windows.GROUP_SECURITY_INFORMATION
		before, err := windows.GetNamedSecurityInfo(target.path, windows.SE_FILE_OBJECT, info)
		if err != nil {
			t.Fatal(err)
		}
		owner, _, err := before.Owner()
		if err != nil || owner == nil || !owner.Equals(currentPrivateUser(t)) {
			t.Fatalf("fresh default-created %s owner differs from current token user; no protection or owner changes attempted", target.kind)
		}
		for _, mode := range []string{"protect", "check", "protect", "check"} {
			var output bytes.Buffer
			if runPrivatePermissions([]string{target.path, target.kind, mode}, &output) != 0 ||
				output.String() != "{\"ok\":true}\n" {
				t.Fatalf("private permissions %s failed: %s", mode, output.String())
			}
		}
		after, err := windows.GetNamedSecurityInfo(target.path, windows.SE_FILE_OBJECT, info)
		if err != nil || before.String() != after.String() {
			t.Fatal("protection changed owner or primary group")
		}
	}
	if privatePermissions(file, true, true) == nil || privatePermissions(dir, false, true) == nil {
		t.Fatal("accepted wrong target type")
	}
	link := filepath.Join(root, "hardlink")
	if err := os.Link(file, link); err != nil {
		t.Fatal(err)
	}
	for _, protect := range []bool{false, true} {
		if privatePermissions(file, false, protect) == nil {
			t.Fatal("accepted multiply-linked file")
		}
	}
}

func TestWindowsPrivatePermissionsRejectReparsePoint(t *testing.T) {
	root := t.TempDir()
	link := filepath.Join(root, "link")
	if err := os.Symlink(root, link); err != nil {
		t.Skipf("symlink creation requires Windows privilege: %v", err)
	}
	for _, protect := range []bool{false, true} {
		if privatePermissions(link, true, protect) == nil {
			t.Fatal("accepted directory symlink")
		}
	}
}

func TestWindowsPrivateDACLPolicy(t *testing.T) {
	user := currentPrivateUser(t)
	sid := user.String()
	for _, test := range []struct {
		name string
		dacl string
		safe bool
	}{
		{"private", "D:P(A;;FA;;;" + sid + ")", true},
		{"read-only", "D:P(A;;FR;;;" + sid + ")", true},
		{"inheritable", "D:P(A;OICI;FA;;;" + sid + ")", true},
		{"unprotected", "D:(A;;FA;;;" + sid + ")", false},
		{"empty", "D:P", false},
		{"null", "D:PNO_ACCESS_CONTROL", false},
		{"missing", "O:" + sid, false},
		{"other-user", "D:P(A;;FA;;;SY)", false},
		{"extra-user", "D:P(A;;FA;;;" + sid + ")(A;;FR;;;WD)", false},
		{"deny", "D:P(D;;FW;;;" + sid + ")(A;;FA;;;" + sid + ")", false},
		{"inherit-only", "D:P(A;OIIO;FA;;;" + sid + ")", false},
		{"object-ace", "D:P(OA;;FA;00112233-4455-6677-8899-aabbccddeeff;;" + sid + ")", false},
	} {
		t.Run(test.name, func(t *testing.T) {
			sd := privateSD(t, test.dacl)
			if safe := checkPrivateDACL(sd, user) == nil; safe != test.safe {
				t.Fatal("DACL policy mismatch")
			}
		})
	}
	for _, owner := range []string{sid, "SY", ""} {
		sddl := "D:P(A;;FA;;;" + sid + ")"
		if owner != "" {
			sddl = "O:" + owner + sddl
		}
		if safe := checkPrivateOwner(privateSD(t, sddl), user) == nil; safe != (owner == sid) {
			t.Fatal("owner policy mismatch")
		}
	}
}

func TestWindowsPrivatePermissionsChecksActualACL(t *testing.T) {
	dir := t.TempDir()
	user := currentPrivateUser(t)
	// Check must reject an actual protected ACL granting another principal.
	sd := privateSD(t, "D:P(A;;FA;;;"+user.String()+")(A;;FR;;;WD)")
	dacl, _, err := sd.DACL()
	if err != nil {
		t.Fatal(err)
	}
	if err := windows.SetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION,
		nil, nil, dacl, nil); err != nil {
		t.Fatal(err)
	}
	before, err := windows.GetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	if privatePermissions(dir, true, false) == nil {
		t.Fatal("check accepted another trustee")
	}
	after, err := windows.GetNamedSecurityInfo(dir, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION)
	if err != nil || before.String() != after.String() {
		t.Fatal("check changed the unsafe ACL instead of rejecting it")
	}
	if err := privatePermissions(dir, true, true); err != nil {
		t.Fatal(err)
	}
	if err := privatePermissions(dir, true, false); err != nil {
		t.Fatal(err)
	}
}

func TestWindowsPrivateACEsRejectMalformedLengths(t *testing.T) {
	user := currentPrivateUser(t)
	sd := privateSD(t, "D:P(A;;FA;;;"+user.String()+")")
	dacl, _, err := sd.DACL()
	if err != nil {
		t.Fatal(err)
	}
	raw := unsafe.Slice((*byte)(unsafe.Pointer(dacl)), int(unsafe.Sizeof(windows.ACL{})))
	size := int(binary.LittleEndian.Uint16(raw[2:4]))
	valid := bytes.Clone(unsafe.Slice((*byte)(unsafe.Pointer(dacl)), size))
	if checkPrivateACEs(valid, user) != nil {
		t.Fatal("rejected native allow ACE")
	}
	for length := 0; length < len(valid); length++ {
		if checkPrivateACEs(valid[:length], user) == nil {
			t.Fatal("accepted truncated ACL")
		}
	}
	for _, mutate := range []func([]byte){
		func(b []byte) { binary.LittleEndian.PutUint16(b[2:4], 65535) },
		func(b []byte) { binary.LittleEndian.PutUint16(b[4:6], 65535) },
		func(b []byte) { binary.LittleEndian.PutUint16(b[10:12], 0) },
		func(b []byte) { binary.LittleEndian.PutUint16(b[10:12], 65535) },
		func(b []byte) { binary.LittleEndian.PutUint16(b[10:12], 8) },
		func(b []byte) { b[17] = 255 }, // SID SubAuthorityCount.
	} {
		bad := bytes.Clone(valid)
		mutate(bad)
		if checkPrivateACEs(bad, user) == nil {
			t.Fatal("accepted malformed ACL")
		}
	}
}

func TestWindowsPrivatePathValidation(t *testing.T) {
	for _, path := range []string{
		"C:relative", "\\relative", `\\?\C:\private`, `\\.\NUL`,
		`C:\private:stream`, `C:\trailing.\file`, `C:\trailing \file`,
	} {
		if validPrivatePath(path) {
			t.Fatal("accepted aliased or non-absolute Windows path")
		}
	}
	for _, path := range []string{`C:\private`, `\\server\share\private`} {
		if !validPrivatePath(path) {
			t.Fatal("rejected normal absolute Windows path")
		}
	}
}
