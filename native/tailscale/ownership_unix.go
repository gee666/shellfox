//go:build darwin || linux

package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/sys/unix"
)

func openOwnershipRoot(directory string)(*ownershipRoot,error) {
	if !filepath.IsAbs(directory) || filepath.Clean(directory)!=directory || directory=="/" {return nil,os.ErrInvalid}
	flags:=unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC
	fd,err:=unix.Open("/",flags,0)
	if err!=nil{return nil,err}
	parts:=strings.Split(strings.TrimPrefix(directory,"/"),"/")
	if len(parts)>256 {_=unix.Close(fd);return nil,os.ErrInvalid}
	for _,part:=range parts {
		if part=="" || part=="." || part==".." {_=unix.Close(fd);return nil,os.ErrInvalid}
		next,openErr:=unix.Openat(fd,part,flags,0)
		_ = unix.Close(fd)
		if openErr!=nil{return nil,openErr}
		fd=next
	}
	return &ownershipRoot{file:os.NewFile(uintptr(fd),directory)},nil
}
func verifyOwnershipUnix(fd int,directory,private bool)error {
	var info unix.Stat_t
	if err:=unix.Fstat(fd,&info);err!=nil{return err}
	kind:=uint32(unix.S_IFREG);mask:=uint32(0177)
	if directory {kind=unix.S_IFDIR;mask=0077;if !private{mask=0022}}
	if uint32(info.Mode)&unix.S_IFMT!=kind || info.Uid!=uint32(os.Getuid()) || (!directory && info.Nlink!=1) || uint32(info.Mode)&mask!=0 {return os.ErrPermission}
	return ownershipACLSafe(fd)
}
func openOwnershipFileExpected(directory,expected string)(*os.File,error) {
	root,err:=openOwnershipRoot(directory)
	if err!=nil{return nil,err}
	defer root.Close()
	// This exact comparison precedes EVERY mutation, including private creation.
	if err:=compareOwnershipIdentity(root.file,expected);err!=nil{return nil,err}
	rootfd:=int(root.file.Fd())
	if err:=verifyOwnershipUnix(rootfd,true,false);err!=nil{return nil,err}
	if err:=unix.Mkdirat(rootfd,ownershipDirectory,0700);err!=nil && !errors.Is(err,unix.EEXIST){return nil,err}
	private,err:=unix.Openat(rootfd,ownershipDirectory,unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC,0)
	if err!=nil{return nil,err}
	defer unix.Close(private)
	if err:=verifyOwnershipUnix(private,true,true);err!=nil{return nil,err}
	flags:=unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NONBLOCK
	fd,err:=unix.Openat(private,ownershipFilename,flags,0)
	if errors.Is(err,unix.ENOENT) {
		// EXCL is the new-object receipt. Never repair an existing object's mode
		// or ACL, even if a contender created it between these two operations.
		fd,err=unix.Openat(private,ownershipFilename,unix.O_RDWR|unix.O_CREAT|unix.O_EXCL|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NONBLOCK,0600)
		if errors.Is(err,unix.EEXIST){fd,err=unix.Openat(private,ownershipFilename,flags,0)}
	}
	if err!=nil{return nil,err}
	file:=os.NewFile(uintptr(fd),filepath.Join(directory,ownershipDirectory,ownershipFilename))
	if err:=verifyOwnershipUnix(fd,false,true);err!=nil{_ = file.Close();return nil,err}
	return file,nil
}
func lockOwnershipFile(file *os.File) (bool, error) {
	for {
		err := unix.Flock(int(file.Fd()), unix.LOCK_EX|unix.LOCK_NB)
		if errors.Is(err, unix.EINTR) {continue}
		if errors.Is(err, unix.EWOULDBLOCK) || errors.Is(err, unix.EAGAIN) {return false,nil}
		return err==nil,err
	}
}
