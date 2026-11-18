package main

import (
	"errors"
	"os"
	"path/filepath"

	"golang.org/x/sys/windows"
)

// The launcher supplies trust from its package/build anchor. This handle does
// not authorize bytes. It pins the actual launched image against write/delete
// sharing until normal helper shutdown has finished.
func acquireRuntimeExecutableLease() (*os.File, error) {
	actual, err := os.Executable()
	if err != nil {
		return nil, errors.New("Cannot identify running helper image")
	}
	return openExecutableReadLease(actual)
}

func openExecutableReadLease(actual string) (*os.File, error) {
	if !filepath.IsAbs(actual) {
		return nil, errors.New("Absolute helper image required")
	}
	name, err := windows.UTF16PtrFromString(actual)
	if err != nil {
		return nil, errors.New("Invalid helper image path")
	}
	handle, err := windows.CreateFile(name, windows.GENERIC_READ|windows.FILE_READ_ATTRIBUTES,
		windows.FILE_SHARE_READ, nil, windows.OPEN_EXISTING,
		windows.FILE_ATTRIBUTE_NORMAL|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return nil, errors.New("Cannot retain helper executable read lease")
	}
	file := os.NewFile(uintptr(handle), actual)
	if file == nil {
		_ = windows.CloseHandle(handle)
		return nil, errors.New("Cannot retain helper image handle")
	}
	if err := validateOwnershipHandle(handle, false); err != nil {
		_ = file.Close()
		return nil, errors.New("Unsafe helper image identity")
	}
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > 256*1024*1024 {
		_ = file.Close()
		return nil, errors.New("Invalid helper image bounds")
	}
	return file, nil
}
