package main

import (
	"errors"
	"os"
	"path/filepath"
	"unsafe"

	"golang.org/x/sys/windows"
)

func ownershipSecurity(directory bool) (*windows.SecurityAttributes, error) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return nil, err
	}
	inheritance := ""
	if directory {
		inheritance = "OICI"
	}
	sd, err := windows.SecurityDescriptorFromString("D:P(A;" + inheritance + ";FA;;;" + user.User.Sid.String() + ")")
	if err != nil {
		return nil, err
	}
	return &windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: sd}, nil
}

func openOwnershipHandle(path string, access, disposition, flags uint32, security *windows.SecurityAttributes) (windows.Handle, error) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return windows.InvalidHandle, err
	}
	// Excluding FILE_SHARE_DELETE pins every opened path during validation.
	return windows.CreateFile(name, access, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE,
		security, disposition, flags|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
}

func validateOwnershipHandle(handle windows.Handle, directory bool) error {
	kind, err := windows.GetFileType(handle)
	if err != nil || kind != windows.FILE_TYPE_DISK {
		return os.ErrInvalid
	}
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		return err
	}
	if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return os.ErrInvalid
	}
	isDirectory := info.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY != 0
	if isDirectory != directory || (!directory && info.NumberOfLinks != 1) {
		return os.ErrInvalid
	}
	return nil
}

func secureOwnershipHandle(handle windows.Handle, security *windows.SecurityAttributes) error {
	dacl, _, err := security.SecurityDescriptor.DACL()
	if err != nil {
		return err
	}
	// Reset only the DACL. Assigning a primary group can require elevation.
	return windows.SetSecurityInfo(handle, windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION,
		nil, nil, dacl, nil)
}

func openOwnershipFile(directory string) (*os.File, error) {
	root, err := openOwnershipHandle(directory, windows.FILE_READ_ATTRIBUTES, windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS, nil)
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(root)
	if err := validateOwnershipHandle(root, true); err != nil {
		return nil, err
	}
	privatePath := filepath.Join(directory, ownershipDirectory)
	dirSecurity, err := ownershipSecurity(true)
	if err != nil {
		return nil, err
	}
	name, err := windows.UTF16PtrFromString(privatePath)
	if err != nil {
		return nil, err
	}
	if err := windows.CreateDirectory(name, dirSecurity); err != nil && !errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
		return nil, err
	}
	private, err := openOwnershipHandle(privatePath, windows.FILE_READ_ATTRIBUTES|windows.READ_CONTROL|windows.WRITE_DAC,
		windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS, nil)
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(private)
	if err := validateOwnershipHandle(private, true); err != nil {
		return nil, err
	}
	if err := secureOwnershipHandle(private, dirSecurity); err != nil {
		return nil, err
	}
	fileSecurity, err := ownershipSecurity(false)
	if err != nil {
		return nil, err
	}
	path := filepath.Join(privatePath, ownershipFilename)
	handle, err := openOwnershipHandle(path, windows.GENERIC_READ|windows.GENERIC_WRITE|windows.READ_CONTROL|windows.WRITE_DAC,
		windows.OPEN_ALWAYS, windows.FILE_ATTRIBUTE_NORMAL, fileSecurity)
	if err != nil {
		return nil, err
	}
	if err := validateOwnershipHandle(handle, false); err != nil {
		windows.CloseHandle(handle)
		return nil, err
	}
	if err := secureOwnershipHandle(handle, fileSecurity); err != nil {
		windows.CloseHandle(handle)
		return nil, err
	}
	return os.NewFile(uintptr(handle), path), nil
}

func lockOwnershipFile(file *os.File) (bool, error) {
	err := windows.LockFileEx(windows.Handle(file.Fd()), windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY,
		0, 1, 0, &windows.Overlapped{})
	if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
		return false, nil
	}
	return err == nil, err
}
