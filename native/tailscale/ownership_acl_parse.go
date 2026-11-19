package main

import "encoding/binary"

// fgetattrlist with only EXTENDED_SECURITY packs a 32-bit reply size, one
// attrreference, and the security data. Darwin packs an empty reference for
// no ACL. A missing/truncated reference is not an absence proof.
func ownershipDarwinACLAbsent(data []byte) bool {
	const (
		fixedSize    = 12
		fileSecSize  = 44
		fileSecMagic = 0x012cc16d
	)
	if len(data) < fixedSize {
		return false
	}
	order := binary.NativeEndian
	size := uint64(order.Uint32(data[:4]))
	length := uint64(order.Uint32(data[8:12]))
	if order.Uint32(data[4:8]) != fixedSize-4 || size != fixedSize+length || size > uint64(len(data)) {
		return false
	}
	if length == 0 {
		return true
	}
	if length != fileSecSize || order.Uint32(data[12:16]) != fileSecMagic {
		return false
	}
	entries := order.Uint32(data[fixedSize+36 : fixedSize+40])
	return entries == 0 || entries == 0xffffffff // KAUTH_FILESEC_NOACL
}
