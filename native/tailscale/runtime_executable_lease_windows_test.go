package main

import (
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/sys/windows"
)

func TestRuntimeReadLeaseDeniesWriteDeleteAndStandardReplacement(t *testing.T) {
	directory := t.TempDir()
	target := filepath.Join(directory, "helper.exe")
	stage := filepath.Join(directory, "stage.exe")
	if err := os.WriteFile(target, []byte("owned version A"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(stage, []byte("owned version B"), 0600); err != nil {
		t.Fatal(err)
	}
	lease, err := openExecutableReadLease(target)
	if err != nil {
		t.Fatal(err)
	}
	name, _ := windows.UTF16PtrFromString(target)
	handle, err := windows.CreateFile(name, windows.GENERIC_WRITE, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, 0, 0)
	if err == nil {
		_ = windows.CloseHandle(handle)
		_ = lease.Close()
		t.Fatal("running read lease permitted a writer")
	}
	if err := windows.DeleteFile(name); err == nil {
		_ = lease.Close()
		t.Fatal("running read lease permitted delete")
	}
	from, _ := windows.UTF16PtrFromString(stage)
	if err := windows.MoveFileEx(from, name, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH); err == nil {
		_ = lease.Close()
		t.Fatal("standard replacement bypassed running read lease")
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	if err := windows.MoveFileEx(from, name, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH); err != nil {
		t.Fatal("standard replacement did not succeed after lease close")
	}
	bytes, err := os.ReadFile(target)
	if err != nil || string(bytes) != "owned version B" {
		t.Fatal("replacement bytes differ")
	}
}

func TestRuntimeReadLeaseRejectsDirectoryAndHardlinkWithoutRepair(t *testing.T) {
	directory := t.TempDir()
	if file, err := openExecutableReadLease(directory); err == nil {
		_ = file.Close()
		t.Fatal("directory image accepted")
	}
	original := filepath.Join(directory, "original.exe")
	alias := filepath.Join(directory, "alias.exe")
	if err := os.WriteFile(original, []byte("owned fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(original, alias); err != nil {
		t.Fatal(err)
	}
	if file, err := openExecutableReadLease(alias); err == nil {
		_ = file.Close()
		t.Fatal("hardlinked image accepted")
	}
	bytes, err := os.ReadFile(original)
	if err != nil || string(bytes) != "owned fixture" {
		t.Fatal("rejected image was modified")
	}
}
