package main

import (
	"os"
	"unsafe"
	"golang.org/x/sys/unix"
)

// Inspect the extended/inherited ACL on the HELD descriptor with the native
// security attribute API. Never infer privacy from uid/mode or chmod. All ACEs,
// including benign deny-only entries, fail closed pending native acceptance.
func ownershipACLSafe(fd int) error {
	attrs:=unix.Attrlist{Bitmapcount:5,Commonattr:unix.ATTR_CMN_RETURNED_ATTRS|unix.ATTR_CMN_EXTENDED_SECURITY}
	var data [4096]byte
	_,_,errno:=unix.Syscall6(unix.SYS_FGETATTRLIST,uintptr(fd),uintptr(unsafe.Pointer(&attrs)),uintptr(unsafe.Pointer(&data[0])),uintptr(len(data)),unix.FSOPT_REPORT_FULLSIZE,0)
	if errno!=0{return errno}
	if !ownershipDarwinACLAbsent(data[:]){return os.ErrPermission}
	return nil
}
