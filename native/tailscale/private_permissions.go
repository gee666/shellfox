package main

import (
	"encoding/json"
	"io"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

// This offline command never reads stdin or starts tsnet. Only fixed messages
// cross the process boundary, not paths, native errors, or security descriptors.
func runPrivatePermissions(args []string, output io.Writer) int {
	message := "Invalid private permissions arguments"
	code := 2
	if len(args) == 3 && validPrivatePath(args[0]) &&
		(args[1] == "file" || args[1] == "directory") &&
		(args[2] == "protect" || args[2] == "check") {
		code = 1
		message = "Unsafe private permissions"
		if err := privatePermissions(args[0], args[1] == "directory", args[2] == "protect"); err == nil {
			if json.NewEncoder(output).Encode(struct {
				OK bool `json:"ok"`
			}{true}) != nil {
				return 1
			}
			return 0
		}
	}
	_ = json.NewEncoder(output).Encode(struct {
		Error string `json:"error"`
	}{message})
	return code
}

func validPrivatePath(path string) bool {
	return len(path) > 0 && len(path) <= 32767 && utf8.ValidString(path) &&
		!strings.ContainsAny(path, "\x00\r\n") && filepath.IsAbs(path) &&
		filepath.Clean(path) == path && validPrivatePlatformPath(path)
}
