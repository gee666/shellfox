package main

import (
	"golang.org/x/sys/unix"
	"os"
	"unsafe"
)

// Inspect the ACL on the held descriptor. Without RETURNED_ATTRS the
// kernel always packs the requested reference, even when there is no ACL.
func ownershipACLSafe(fd int) error {
	attrs := unix.Attrlist{Bitmapcount: 5, Commonattr: unix.ATTR_CMN_EXTENDED_SECURITY}
	var data [4096]byte
	_, _, errno := unix.Syscall6(unix.SYS_FGETATTRLIST, uintptr(fd), uintptr(unsafe.Pointer(&attrs)), uintptr(unsafe.Pointer(&data[0])), uintptr(len(data)), unix.FSOPT_REPORT_FULLSIZE, 0)
	if errno != 0 {
		return errno
	}
	if !ownershipDarwinACLAbsent(data[:]) {
		return os.ErrPermission
	}
	return nil
}
