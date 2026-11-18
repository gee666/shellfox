package main

import (
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"runtime"
	"strings"
)

type ownershipRoot struct {
	file *os.File
	ancestors []*os.File
}
func (r *ownershipRoot) Close() {
	_ = r.file.Close()
	for _, file := range r.ancestors { _ = file.Close() }
}
// The representation is OS-tagged and exact-width. No path or legacy file ID
// can substitute for the identity captured from the caller's original handle.
func validateOwnershipIdentity(value string) error {
	parts := strings.Split(value, ":")
	if len(parts) != 4 || parts[0] != "root-v1" || parts[1] != runtime.GOOS { return os.ErrInvalid }
	widths := []int{16,16}
	switch runtime.GOOS {
	case "linux":
	case "darwin": widths[0]=8
	case "windows": widths[1]=32
	default: return errors.New("unsupported ownership identity provider")
	}
	for i,width := range widths {
		raw := parts[i+2]
		if len(raw)!=width || strings.ToLower(raw)!=raw {return os.ErrInvalid}
		if _,err:=hex.DecodeString(raw);err!=nil{return os.ErrInvalid}
	}
	return nil
}
func compareOwnershipIdentity(file *os.File, expected string) error {
	if err:=validateOwnershipIdentity(expected);err!=nil{return err}
	actual,err:=ownershipIdentity(file)
	if err!=nil{return err}
	if actual!=expected{return fmt.Errorf("original ownership root changed")}
	return nil
}
