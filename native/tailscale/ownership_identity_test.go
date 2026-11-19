package main

import (
	"encoding/binary"
	"runtime"
	"strings"
	"testing"
)

func TestOwnershipIdentityStrictNativeWidthRepresentation(t *testing.T) {
	first, second := 16, 16
	if runtime.GOOS == "darwin" {
		first = 8
	}
	if runtime.GOOS == "windows" {
		second = 32
	}
	valid := "root-v1:" + runtime.GOOS + ":" + strings.Repeat("f", first) + ":" + strings.Repeat("a", second)
	if err := validateOwnershipIdentity(valid); err != nil {
		t.Fatal("exact native identity rejected")
	}
	for _, bad := range []string{strings.Replace(valid, "root-v1:", "root-v2:", 1), strings.Replace(valid, runtime.GOOS, "other", 1), strings.ToUpper(valid), valid + ":extra", valid[:len(valid)-1], strings.Replace(valid, ":"+strings.Repeat("f", first)+":", ":1:", 1)} {
		if validateOwnershipIdentity(bad) == nil {
			t.Fatal("ambiguous or downgraded identity accepted")
		}
	}
}
func TestOwnershipDarwinACLRequiresExactNativeReplyAndNoEntries(t *testing.T) {
	data := make([]byte, 56)
	put := func(at int, value uint32) { binary.NativeEndian.PutUint32(data[at:at+4], value) }
	put(0, 12)
	put(4, 8)
	if !ownershipDarwinACLAbsent(data) {
		t.Fatal("explicit zero-length ACL rejected")
	}
	if ownershipDarwinACLAbsent(data[:4]) {
		t.Fatal("missing ACL reference became proof")
	}
	put(0, 56)
	put(8, 44)
	put(12, 0x012cc16d)
	if !ownershipDarwinACLAbsent(data) {
		t.Fatal("empty native ACL rejected")
	}
	put(48, 1)
	if ownershipDarwinACLAbsent(data) {
		t.Fatal("ACE/inheritance guessed safe")
	}
	put(48, 0xffffffff)
	if !ownershipDarwinACLAbsent(data) {
		t.Fatal("native NOACL sentinel rejected")
	}
	put(4, 0x80000000)
	if ownershipDarwinACLAbsent(data) {
		t.Fatal("invalid relative range accepted")
	}
}
