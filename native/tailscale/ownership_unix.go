//go:build darwin || linux

package main

import (
	"errors"
	"os"
	"path/filepath"

	"golang.org/x/sys/unix"
)

func openOwnershipFile(directory string) (*os.File, error) {
	// Anchor each lookup to an opened directory and never follow a final symlink.
	// Only the private lock directory's permissions change, not the user's appdata.
	root, err := unix.Open(directory, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	defer unix.Close(root)
	if err := unix.Mkdirat(root, ownershipDirectory, 0700); err != nil && !errors.Is(err, unix.EEXIST) {
		return nil, err
	}
	private, err := unix.Openat(root, ownershipDirectory, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	defer unix.Close(private)
	if err := unix.Fchmod(private, 0700); err != nil {
		return nil, err
	}
	fd, err := unix.Openat(private, ownershipFilename, unix.O_RDWR|unix.O_CREAT|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NONBLOCK, 0600)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(fd), filepath.Join(directory, ownershipDirectory, ownershipFilename))
	var info unix.Stat_t
	if err := unix.Fstat(fd, &info); err != nil || info.Mode&unix.S_IFMT != unix.S_IFREG || info.Nlink != 1 {
		_ = file.Close()
		return nil, os.ErrInvalid
	}
	if err := file.Chmod(0600); err != nil {
		_ = file.Close()
		return nil, err
	}
	return file, nil
}

func lockOwnershipFile(file *os.File) (bool, error) {
	for {
		err := unix.Flock(int(file.Fd()), unix.LOCK_EX|unix.LOCK_NB)
		if errors.Is(err, unix.EINTR) {
			continue
		}
		if errors.Is(err, unix.EWOULDBLOCK) || errors.Is(err, unix.EAGAIN) {
			return false, nil
		}
		return err == nil, err
	}
}
