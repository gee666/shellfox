//go:build darwin || linux

package main

import (
	"os"
	"path/filepath"
	"testing"
)

// Source-only regression coverage until native test execution is authorized.
func TestBoundOwnershipRejectsRootReplacementBeforeAnyMutation(t *testing.T) {
	parent:=t.TempDir();path:=filepath.Join(parent,"root")
	if err:=os.Mkdir(path,0700);err!=nil{t.Fatal(err)}
	held,err:=openOwnershipRoot(path);if err!=nil{t.Fatal(err)};defer held.Close()
	expected,err:=ownershipIdentity(held.file);if err!=nil{t.Fatal(err)}
	old:=filepath.Join(parent,"retained")
	if err:=os.Rename(path,old);err!=nil{t.Fatal(err)}
	if err:=os.Mkdir(path,0700);err!=nil{t.Fatal(err)}
	if file,locked,err:=acquireOwnershipExpected(path,expected);err==nil || locked || file!=nil{t.Fatal("replaced root admitted")}
	for _,root:=range []string{path,old}{if _,err:=os.Lstat(filepath.Join(root,ownershipDirectory));!os.IsNotExist(err){t.Fatal("root comparison happened after mutation")}}
}
func TestBoundOwnershipRejectsSymlinkAncestorWithoutTouchingTarget(t *testing.T) {
	parent:=t.TempDir();real:=filepath.Join(parent,"real");path:=filepath.Join(real,"root")
	if err:=os.MkdirAll(path,0700);err!=nil{t.Fatal(err)}
	held,err:=openOwnershipRoot(path);if err!=nil{t.Fatal(err)};defer held.Close()
	expected,err:=ownershipIdentity(held.file);if err!=nil{t.Fatal(err)}
	alias:=filepath.Join(parent,"alias");if err:=os.Symlink(real,alias);err!=nil{t.Fatal(err)}
	if file,locked,err:=acquireOwnershipExpected(filepath.Join(alias,"root"),expected);err==nil || locked || file!=nil{t.Fatal("ancestor alias admitted")}
	if _,err:=os.Lstat(filepath.Join(path,ownershipDirectory));!os.IsNotExist(err){t.Fatal("symlink target mutated")}
}
func TestOccupiedOwnershipNeverRepairsExistingPermissions(t *testing.T) {
	path:=t.TempDir();if err:=os.Chmod(path,0700);err!=nil{t.Fatal(err)}
	file,locked,err:=acquireOwnership(path);if err!=nil || !locked{t.Fatal("fixture owner unavailable",err)};defer file.Close()
	private:=filepath.Join(path,ownershipDirectory);lock:=filepath.Join(private,ownershipFilename)
	if err:=os.Chmod(private,0755);err!=nil{t.Fatal(err)};if err:=os.Chmod(lock,0644);err!=nil{t.Fatal(err)}
	other,acquired,err:=acquireOwnership(path);if other!=nil{_ = other.Close()}
	if err==nil || acquired{t.Fatal("unsafe occupied metadata was repaired/admitted")}
	for name,mode:=range map[string]os.FileMode{private:0755,lock:0644}{info,e:=os.Stat(name);if e!=nil || info.Mode().Perm()!=mode{t.Fatal("contender changed existing permissions")}}
}
