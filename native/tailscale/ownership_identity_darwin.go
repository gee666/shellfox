package main

import (
	"fmt"
	"os"
	"golang.org/x/sys/unix"
)
func ownershipIdentity(file *os.File)(string,error) {
	var info unix.Stat_t
	if err:=unix.Fstat(int(file.Fd()),&info);err!=nil{return "",err}
	// dev_t is signed32 on Darwin. uint32 preserves its EXACT native bits,
	// including the sign bit; it is not a truncation of a64-bit provider.
	return fmt.Sprintf("root-v1:darwin:%08x:%016x",uint32(info.Dev),info.Ino),nil
}
