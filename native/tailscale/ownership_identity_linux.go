package main

import (
	"fmt"
	"os"
	"golang.org/x/sys/unix"
)
func ownershipIdentity(file *os.File)(string,error) {
	var info unix.Stat_t
	if err:=unix.Fstat(int(file.Fd()),&info);err!=nil{return "",err}
	return fmt.Sprintf("root-v1:linux:%016x:%016x",info.Dev,info.Ino),nil
}
