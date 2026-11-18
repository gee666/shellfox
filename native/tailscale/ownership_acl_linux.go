package main

import (
	"errors"
	"os"
	"golang.org/x/sys/unix"
)
// Never infer extended-ACL safety from mode bits or chmod. Reject access and
// inherited/default ACLs; unsupported inspection fails closed.
func ownershipACLSafe(fd int) error {
	for _,name:=range []string{"system.posix_acl_access","system.posix_acl_default"} {
		_,err:=unix.Fgetxattr(fd,name,nil)
		if errors.Is(err,unix.ENODATA){continue}
		if err!=nil{return err}
		return os.ErrPermission
	}
	return nil
}
