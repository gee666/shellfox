package main

import (
	"bytes"
	"encoding/binary"
	"os"
	"path/filepath"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

func validPrivatePlatformPath(path string) bool {
	volume := filepath.VolumeName(path)
	// Reject device namespaces and alternate data streams. Accept ordinary
	// drive-absolute and UNC paths only, without Win32 trailing-dot aliases.
	drive := len(volume) == 2 && volume[1] == ':' &&
		((volume[0] >= 'A' && volume[0] <= 'Z') || (volume[0] >= 'a' && volume[0] <= 'z'))
	unc := strings.HasPrefix(volume, `\\`) &&
		!strings.HasPrefix(volume, `\\?\`) && !strings.HasPrefix(volume, `\\.\`)
	if (!drive && !unc) || strings.Contains(path[len(volume):], ":") {
		return false
	}
	for _, part := range strings.Split(path, `\`) {
		if strings.HasSuffix(part, ".") || strings.HasSuffix(part, " ") {
			return false
		}
	}
	return true
}

func privatePermissions(path string, directory, protect bool) error {
	access := uint32(windows.READ_CONTROL | windows.FILE_READ_ATTRIBUTES)
	if protect {
		access |= windows.WRITE_DAC
	}
	handle, err := openOwnershipHandle(path, access, windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS, nil)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	if err := validateOwnershipHandle(handle, directory); err != nil {
		return err
	}
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return err
	}
	sd, err := privateSecurityDescriptor(handle, user.User.Sid)
	if err != nil {
		return err
	}
	if protect {
		security, err := ownershipSecurity(directory)
		if err != nil {
			return err
		}
		if err := secureOwnershipHandle(handle, security); err != nil {
			return err
		}
		sd, err = privateSecurityDescriptor(handle, user.User.Sid)
		if err != nil {
			return err
		}
	}
	return checkPrivateDACL(sd, user.User.Sid)
}

func privateSecurityDescriptor(handle windows.Handle, user *windows.SID) (*windows.SECURITY_DESCRIPTOR, error) {
	sd, err := windows.GetSecurityInfo(handle, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		return nil, err
	}
	if err := checkPrivateOwner(sd, user); err != nil {
		return nil, err
	}
	return sd, nil
}

func checkPrivateOwner(sd *windows.SECURITY_DESCRIPTOR, user *windows.SID) error {
	if sd == nil || !sd.IsValid() {
		return os.ErrPermission
	}
	owner, _, err := sd.Owner()
	if err != nil || owner == nil || !owner.IsValid() || !owner.Equals(user) {
		return os.ErrPermission
	}
	return nil
}

func checkPrivateDACL(sd *windows.SECURITY_DESCRIPTOR, user *windows.SID) error {
	control, _, err := sd.Control()
	if err != nil || control&windows.SE_DACL_PROTECTED == 0 {
		return os.ErrPermission
	}
	acl, _, err := sd.DACL()
	if err != nil || acl == nil {
		return os.ErrPermission
	}
	// GetSecurityInfo returns a self-relative descriptor on the Go heap.
	// Bound the ACL by that allocation before reading its header or any ACE.
	base := uintptr(unsafe.Pointer(sd))
	start := uintptr(unsafe.Pointer(acl))
	length := uintptr(sd.Length())
	headerSize := uintptr(unsafe.Sizeof(windows.ACL{}))
	if start < base || start-base > length || length-(start-base) < headerSize {
		return os.ErrPermission
	}
	data := unsafe.Slice((*byte)(unsafe.Pointer(acl)), int(length-(start-base)))
	// Win32 ACL's uint16 AclSize is at byte 2, followed by uint16 AceCount.
	size := int(binary.LittleEndian.Uint16(data[2:4]))
	if size < int(headerSize) || size > len(data) {
		return os.ErrPermission
	}
	return checkPrivateACEs(data[:size], user)
}

func checkPrivateACEs(acl []byte, user *windows.SID) error {
	headerSize := int(unsafe.Sizeof(windows.ACL{}))
	if len(acl) < headerSize || int(binary.LittleEndian.Uint16(acl[2:4])) != len(acl) {
		return os.ErrPermission
	}
	count := int(binary.LittleEndian.Uint16(acl[4:6]))
	if count == 0 {
		return os.ErrPermission
	}
	sidOffset := int(unsafe.Offsetof(windows.ACCESS_ALLOWED_ACE{}.SidStart))
	sid := unsafe.Slice((*byte)(unsafe.Pointer(user)), user.Len())
	offset := headerSize
	for range count {
		if len(acl)-offset < sidOffset {
			return os.ErrPermission
		}
		ace := acl[offset:]
		size := int(binary.LittleEndian.Uint16(ace[2:4]))
		if size < sidOffset || size > len(ace) {
			return os.ErrPermission
		}
		ace = ace[:size]
		// Only ordinary allow ACEs for the exact current-user SID are accepted.
		// Byte comparison validates the entire SID length before any SID API
		// could read untrusted ACE memory. Object/callback/deny ACEs fail closed.
		if ace[0] != windows.ACCESS_ALLOWED_ACE_TYPE ||
			ace[1]&windows.INHERIT_ONLY_ACE != 0 ||
			binary.LittleEndian.Uint32(ace[4:8]) == 0 ||
			!bytes.Equal(ace[sidOffset:], sid) {
			return os.ErrPermission
		}
		offset += size
	}
	return nil
}
