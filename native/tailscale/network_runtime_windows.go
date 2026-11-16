package main

import (
	"crypto/sha256"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"golang.org/x/sys/windows"
)

const networkRuntimeName = "shellfox-tailscale.exe"

func launchNetworkRuntime(channel string) (int, error) {
	source, err := os.Executable()
	if err != nil {
		return 1, err
	}
	arch := map[string]string{"amd64": "x64", "arm64": "arm64"}[runtime.GOARCH]
	return launchNetworkRuntimeFrom(source, os.Getenv("LOCALAPPDATA"), arch, channel, nil)
}

func launchNetworkRuntimeFrom(source, local, arch, channel string, args []string) (int, error) {
	root, handles, err := prepareNetworkRoot(local, arch, channel)
	defer closeNetworkHandles(handles)
	if err != nil {
		return 1, err
	}
	lock, err := waitNetworkLock(root, 5*time.Second)
	if err != nil {
		return 1, err
	}
	defer lock.Close()
	executable, err := stageNetworkExecutable(source, root)
	if err != nil {
		return 1, err
	}
	// Pin the verified bytes against writes and deletion through the child's lifetime.
	defer executable.Close()
	child, err := startNetworkProcess(executable.Name(), args, os.Stdin, os.Stdout, os.Stderr)
	if err != nil {
		return 1, err
	}
	defer child.close()
	// The live child and our read handle now prevent another generation replacing it.
	_ = lock.Close()
	return child.wait()
}

func closeNetworkHandles(handles []windows.Handle) {
	for i := len(handles) - 1; i >= 0; i-- {
		_ = windows.CloseHandle(handles[i])
	}
}

// Pin all path components while inspecting them. Never change the app install
// root's ACL; only the dedicated network tree receives a protected private DACL.
func prepareNetworkRoot(local, arch, channel string) (string, []windows.Handle, error) {
	volume := filepath.VolumeName(local)
	if !validPrivatePath(local) || len(volume) != 2 || volume[1] != ':' || (arch != "x64" && arch != "arm64") || (channel != "" && channel != "testing") {
		return "", nil, os.ErrInvalid
	}
	drive, err := windows.UTF16PtrFromString(volume + `\`)
	if err != nil || windows.GetDriveType(drive) != windows.DRIVE_FIXED {
		return "", nil, os.ErrInvalid
	}
	var handles []windows.Handle
	paths := []string{local, filepath.Join(local, "Shellfox"), filepath.Join(local, "Shellfox", "network"), filepath.Join(local, "Shellfox", "network", arch)}
	if channel == "testing" {
		paths = append(paths, filepath.Join(paths[len(paths)-1], channel))
	}
	for i, path := range paths {
		if i > 0 {
			if err := os.Mkdir(path, 0700); err != nil && !errors.Is(err, os.ErrExist) {
				return "", handles, err
			}
		}
		handle, err := openOwnershipHandle(path, windows.FILE_READ_ATTRIBUTES|windows.FILE_LIST_DIRECTORY, windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS, nil)
		if err != nil {
			return "", handles, err
		}
		handles = append(handles, handle)
		if err := validateOwnershipHandle(handle, true); err != nil {
			return "", handles, err
		}
		if i >= 2 {
			if err := privatePermissions(path, true, true); err != nil {
				return "", handles, err
			}
		}
	}
	return paths[len(paths)-1], handles, nil
}

func waitNetworkLock(root string, timeout time.Duration) (*os.File, error) {
	deadline := time.Now().Add(timeout)
	for {
		file, locked, err := acquireOwnership(root)
		if err != nil {
			return nil, err
		}
		if locked {
			return file, nil
		}
		if !time.Now().Before(deadline) {
			return nil, os.ErrDeadlineExceeded
		}
		time.Sleep(25 * time.Millisecond)
	}
}

func openNetworkExecutable(path string, protect bool) (*os.File, error) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil, err
	}
	access := uint32(windows.GENERIC_READ)
	if protect {
		access |= windows.READ_CONTROL | windows.WRITE_DAC
	}
	// Unlike os.Open, this handle denies both writes and renames/deletion.
	handle, err := windows.CreateFile(name, access, windows.FILE_SHARE_READ, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(handle), path)
	if err := validateOwnershipHandle(handle, false); err != nil {
		_ = file.Close()
		return nil, err
	}
	if protect {
		user, err := windows.GetCurrentProcessToken().GetTokenUser()
		if err == nil {
			_, err = privateSecurityDescriptor(handle, user.User.Sid)
		}
		if err == nil {
			var security *windows.SecurityAttributes
			security, err = ownershipSecurity(false)
			if err == nil {
				err = secureOwnershipHandle(handle, security)
			}
		}
		if err != nil {
			_ = file.Close()
			return nil, err
		}
	}
	return file, nil
}

func networkDigest(file *os.File) ([sha256.Size]byte, error) {
	var result [sha256.Size]byte
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return result, err
	}
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return result, err
	}
	copy(result[:], hash.Sum(nil))
	return result, nil
}

// Caller holds the deployment lock until process creation has completed.
func stageNetworkExecutable(source, root string) (*os.File, error) {
	input, err := openNetworkExecutable(source, false)
	if err != nil {
		return nil, err
	}
	defer input.Close()
	want, err := networkDigest(input)
	if err != nil {
		return nil, err
	}
	destination := filepath.Join(root, networkRuntimeName)
	current, err := openNetworkExecutable(destination, true)
	if err == nil {
		got, hashErr := networkDigest(current)
		if hashErr == nil && got == want {
			return current, nil
		}
		_ = current.Close()
		if hashErr != nil {
			return nil, hashErr
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	partial, err := os.CreateTemp(root, ".network-*.tmp")
	if err != nil {
		return nil, err
	}
	defer os.Remove(partial.Name())
	defer partial.Close()
	if _, err := input.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}
	if _, err := io.Copy(partial, input); err != nil {
		return nil, err
	}
	got, err := networkDigest(partial)
	if err != nil || got != want {
		return nil, os.ErrInvalid
	}
	if err := partial.Sync(); err != nil {
		return nil, err
	}
	if err := privatePermissions(partial.Name(), false, true); err != nil {
		return nil, err
	}
	if err := partial.Close(); err != nil {
		return nil, err
	}
	// Windows refuses replacement while another launcher or running image pins
	// the old executable. Never delete it first or run a different-path fallback.
	from, err := windows.UTF16PtrFromString(partial.Name())
	if err != nil {
		return nil, err
	}
	to, err := windows.UTF16PtrFromString(destination)
	if err != nil {
		return nil, err
	}
	if err := windows.MoveFileEx(from, to, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH); err != nil {
		return nil, err
	}
	installed, err := openNetworkExecutable(destination, true)
	if err != nil {
		return nil, err
	}
	got, err = networkDigest(installed)
	if err != nil || got != want {
		_ = installed.Close()
		return nil, os.ErrInvalid
	}
	return installed, nil
}
