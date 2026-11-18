package main

import "encoding/binary"

// Native Darwin fgetattrlist reply, requested RETURNED_ATTRS + EXTENDED_SECURITY.
// Reject every ACE, even benign deny-only/inherited rules. An unreturned or
// truncated security attribute is never absence proof.
func ownershipDarwinACLAbsent(data []byte) bool {
	if len(data)<32{return false}
	order:=binary.NativeEndian
	size:=uint64(order.Uint32(data[:4]))
	if size<32 || size>uint64(len(data)){return false}
	common:=order.Uint32(data[4:8])
	if common&0x00400000==0 || common & ^uint32(0x80400000)!=0{return false}
	for offset:=8;offset<24;offset+=4{if order.Uint32(data[offset:offset+4])!=0{return false}}
	relative:=int64(int32(order.Uint32(data[24:28])))
	length:=uint64(order.Uint32(data[28:32]))
	start:=int64(24)+relative
	if start<0 || uint64(start)>size || length>size-uint64(start){return false}
	if length==0{return true}
	if length<44{return false}
	security:=data[uint64(start):uint64(start)+length]
	if order.Uint32(security[:4])!=0x012cc16d{return false}
	entries:=order.Uint32(security[36:40])
	return entries==0 || entries==0xffffffff
}
