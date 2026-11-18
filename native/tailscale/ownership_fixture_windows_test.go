package main

import (
	"golang.org/x/sys/windows"
	"runtime"
)

func createOwnershipTestDirectory(path string) error {
	security, err := ownershipSecurity(true)
	if err != nil {
		return err
	}
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return err
	}
	err = windows.CreateDirectory(name, security)
	runtime.KeepAlive(security)
	return err
}
