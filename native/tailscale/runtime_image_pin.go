package main

import (
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"io"
	"os"
)

const maximumRuntimeImageBytes = 256 * 1024 * 1024

// Existing offline/probe/private arguments are handled unchanged by main.
// Only the normal network mode gains this optional externally supplied pin.
func runtimeNetworkArguments(args []string) (expected []byte, network bool, err error) {
	if len(args) == 0 {
		return nil, true, nil
	}
	if args[0] != "--expected-sha256" {
		return nil, false, nil
	}
	if len(args) != 2 || len(args[1]) != sha256.Size*2 {
		return nil, false, errors.New("Invalid expected helper digest argument")
	}
	decoded, err := hex.DecodeString(args[1])
	if err != nil || len(decoded) != sha256.Size {
		return nil, false, errors.New("Invalid expected helper digest argument")
	}
	return decoded, true, nil
}

// Hash exactly the same opened descriptor whose sharing lease stays held. The
// expected bytes come from the caller's trusted package/build anchor, never a
// value computed from this image and then promoted to authority.
func verifyRuntimeImagePin(file *os.File, expected []byte) error {
	if file == nil || len(expected) != sha256.Size {
		return errors.New("Expected helper image pin is unavailable")
	}
	before, err := file.Stat()
	if err != nil || !before.Mode().IsRegular() || before.Size() <= 0 || before.Size() > maximumRuntimeImageBytes {
		return errors.New("Invalid helper image bounds")
	}
	hash := sha256.New()
	n, err := io.Copy(hash, io.NewSectionReader(file, 0, maximumRuntimeImageBytes+1))
	if err != nil || n != before.Size() || n <= 0 || n > maximumRuntimeImageBytes {
		return errors.New("Cannot verify complete helper image")
	}
	after, err := file.Stat()
	if err != nil || !os.SameFile(before, after) || after.Size() != before.Size() || !after.ModTime().Equal(before.ModTime()) {
		return errors.New("Helper image changed during verification")
	}
	if subtle.ConstantTimeCompare(hash.Sum(nil), expected) != 1 {
		return errors.New("Helper image differs from expected digest")
	}
	return nil
}
