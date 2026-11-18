package main

import (
	"fmt"
	"os"
	"unsafe"
	"golang.org/x/sys/windows"
)
type ownershipFileIDInfo struct {
	VolumeSerialNumber uint64
	FileID [16]byte
}
func ownershipIdentity(file *os.File)(string,error) {
	var info ownershipFileIDInfo
	// FILE_ID_INFO is volume64 plus the FULL native FILE_ID_128. ReFS is not
	// truncated to ByHandleFileInformation's legacy64-bit index.
	if err:=windows.GetFileInformationByHandleEx(windows.Handle(file.Fd()),18,(*byte)(unsafe.Pointer(&info)),uint32(unsafe.Sizeof(info)));err!=nil{return "",err}
	if info.FileID==[16]byte{} {return "",os.ErrInvalid}
	return fmt.Sprintf("root-v1:windows:%016x:%x",info.VolumeSerialNumber,info.FileID),nil
}
