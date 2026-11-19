package main

import (
	"errors"
	"os"
	"path/filepath"
 "strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

func ownershipSecurity(directory bool) (*windows.SecurityAttributes, error) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return nil, err
	}
	inheritance := ""
	if directory {
		inheritance = "OICI"
	}
	sd, err := windows.SecurityDescriptorFromString("D:P(A;" + inheritance + ";FA;;;" + user.User.Sid.String() + ")")
	if err != nil {
		return nil, err
	}
	return &windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: sd}, nil
}

func openOwnershipHandle(path string, access, disposition, flags uint32, security *windows.SecurityAttributes) (windows.Handle, error) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return windows.InvalidHandle, err
	}
	// Excluding FILE_SHARE_DELETE pins every opened path during validation.
	return windows.CreateFile(name, access, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE,
		security, disposition, flags|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
}

func validateOwnershipHandle(handle windows.Handle, directory bool) error {
	kind, err := windows.GetFileType(handle)
	if err != nil || kind != windows.FILE_TYPE_DISK {
		return os.ErrInvalid
	}
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		return err
	}
	if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return os.ErrInvalid
	}
	isDirectory := info.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY != 0
	if isDirectory != directory || (!directory && info.NumberOfLinks != 1) {
		return os.ErrInvalid
	}
	return nil
}

func secureOwnershipHandle(handle windows.Handle, security *windows.SecurityAttributes) error {
	dacl, _, err := security.SecurityDescriptor.DACL()
	if err != nil {
		return err
	}
	// Reset only the DACL. Assigning a primary group can require elevation.
	return windows.SetSecurityInfo(handle, windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION,
		nil, nil, dacl, nil)
}

func openOwnershipRoot(directory string)(*ownershipRoot,error) {
 if !validPrivatePath(directory){return nil,os.ErrInvalid}
 volume:=filepath.VolumeName(directory)
 suffix:=strings.TrimPrefix(directory[len(volume):], `\`)
 if suffix=="" {return nil,os.ErrInvalid}
 paths:=[]string{volume+`\`}
 parts:=strings.Split(suffix,`\`)
 if len(parts)>256{return nil,os.ErrInvalid}
 for _,part:=range parts {
  if part=="" || part=="." || part==".." {return nil,os.ErrInvalid}
  paths=append(paths,filepath.Join(paths[len(paths)-1],part))
 }
 files:=[]*os.File{}
 for _,path:=range paths {
  handle,err:=openOwnershipHandle(path,windows.FILE_READ_ATTRIBUTES|windows.READ_CONTROL,windows.OPEN_EXISTING,windows.FILE_FLAG_BACKUP_SEMANTICS,nil)
  if err!=nil {for _,file:=range files{_ = file.Close()};return nil,err}
  file:=os.NewFile(uintptr(handle),path)
  if err:=validateOwnershipHandle(handle,true);err!=nil{_ = file.Close();for _,old:=range files{_ = old.Close()};return nil,err}
  files=append(files,file)
 }
 return &ownershipRoot{file:files[len(files)-1],ancestors:files[:len(files)-1]},nil
}
func verifyOwnershipPrivate(handle windows.Handle,directory bool)error {
 if err:=validateOwnershipHandle(handle,directory);err!=nil{return err}
 user,err:=windows.GetCurrentProcessToken().GetTokenUser();if err!=nil{return err}
 sd,err:=privateSecurityDescriptor(handle,user.User.Sid);if err!=nil{return err}
 return checkPrivateDACL(sd,user.User.Sid,directory)
}
func openOwnershipFileExpected(directory,expected string)(*os.File,error) {
 root,err:=openOwnershipRoot(directory);if err!=nil{return nil,err};defer root.Close()
 if err:=compareOwnershipIdentity(root.file,expected);err!=nil{return nil,err}
 // Existing data and lock security is verified only. A contender must never
 // rewrite a DACL, chmod a file, or replace an old private directory.
 if err:=verifyOwnershipPrivate(windows.Handle(root.file.Fd()),true);err!=nil{return nil,err}
 privatePath:=filepath.Join(directory,ownershipDirectory)
 dirSecurity,err:=ownershipSecurity(true);if err!=nil{return nil,err}
 name,err:=windows.UTF16PtrFromString(privatePath);if err!=nil{return nil,err}
 if err:=windows.CreateDirectory(name,dirSecurity);err!=nil && !errors.Is(err,windows.ERROR_ALREADY_EXISTS){return nil,err}
 private,err:=openOwnershipHandle(privatePath,windows.FILE_READ_ATTRIBUTES|windows.READ_CONTROL,windows.OPEN_EXISTING,windows.FILE_FLAG_BACKUP_SEMANTICS,nil)
 if err!=nil{return nil,err};defer windows.CloseHandle(private)
 if err:=verifyOwnershipPrivate(private,true);err!=nil{return nil,err}
 path:=filepath.Join(privatePath,ownershipFilename)
 handle,err:=openOwnershipHandle(path,windows.GENERIC_READ|windows.READ_CONTROL,windows.OPEN_EXISTING,windows.FILE_ATTRIBUTE_NORMAL,nil)
 if errors.Is(err,windows.ERROR_FILE_NOT_FOUND) {
  security,secErr:=ownershipSecurity(false);if secErr!=nil{return nil,secErr}
  handle,err=openOwnershipHandle(path,windows.GENERIC_READ|windows.GENERIC_WRITE|windows.READ_CONTROL,windows.CREATE_NEW,windows.FILE_ATTRIBUTE_NORMAL,security)
  if errors.Is(err,windows.ERROR_FILE_EXISTS) || errors.Is(err,windows.ERROR_ALREADY_EXISTS){handle,err=openOwnershipHandle(path,windows.GENERIC_READ|windows.READ_CONTROL,windows.OPEN_EXISTING,windows.FILE_ATTRIBUTE_NORMAL,nil)}
 }
 if err!=nil{return nil,err}
 if err:=verifyOwnershipPrivate(handle,false);err!=nil{windows.CloseHandle(handle);return nil,err}
 return os.NewFile(uintptr(handle),path),nil
}

func lockOwnershipFile(file *os.File) (bool, error) {
	err := windows.LockFileEx(windows.Handle(file.Fd()), windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY,
		0, 1, 0, &windows.Overlapped{})
	if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
		return false, nil
	}
	return err == nil, err
}
