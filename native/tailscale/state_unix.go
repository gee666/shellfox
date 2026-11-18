//go:build darwin || linux

package main

import (
	"os"
	"path/filepath"
	"golang.org/x/sys/unix"
)

// Node-state protection is descriptor anchored and ACL-aware. chmod alone is
// never considered proof that inherited/extended grants disappeared.
func protectStatePath(path string, directory bool) error {
	var file *os.File
	if directory {
		root,err:=openOwnershipRoot(path);if err!=nil{return err};defer root.Close();file=root.file
	} else {
		root,err:=openOwnershipRoot(filepath.Dir(path));if err!=nil{return err};defer root.Close()
		fd,err:=unix.Openat(int(root.file.Fd()),filepath.Base(path),unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NONBLOCK,0)
		if err!=nil{return err};file=os.NewFile(uintptr(fd),path);defer file.Close()
	}
	fd:=int(file.Fd());var info unix.Stat_t
	if err:=unix.Fstat(fd,&info);err!=nil{return err}
	kind:=uint32(unix.S_IFREG);mode:=uint32(0600)
	if directory{kind=unix.S_IFDIR;mode=0700}
	if uint32(info.Mode)&unix.S_IFMT!=kind || info.Uid!=uint32(os.Getuid()) || (!directory && info.Nlink!=1){return os.ErrPermission}
	if err:=ownershipACLSafe(fd);err!=nil{return err}
	return unix.Fchmod(fd,mode)
}
