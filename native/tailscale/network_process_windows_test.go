package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestNetworkLauncherProcessEntry(t *testing.T) {
	local := os.Getenv("SHELLFOX_NETWORK_TEST_LOCAL")
	if local == "" {
		t.Skip("subprocess entry point")
	}
	code, err := launchNetworkRuntimeFrom(os.Args[0], local, "x64", os.Getenv("SHELLFOX_NETWORK_TEST_CHANNEL"), []string{"-test.run=^TestNetworkChildProcessEntry$"})
	if err != nil {
		fmt.Fprintln(os.Stderr, networkRuntimeFailure)
		os.Exit(1)
	}
	os.Exit(code)
}

func TestNetworkChildProcessEntry(t *testing.T) {
	if os.Getenv("SHELLFOX_NETWORK_TEST_LOCAL") == "" {
		t.Skip("subprocess entry point")
	}
	executable, _ := os.Executable()
	console, _, _ := windows.NewLazySystemDLL("kernel32.dll").NewProc("GetConsoleWindow").Call()
	_ = json.NewEncoder(os.Stdout).Encode(struct {
		PID        int    `json:"pid"`
		Executable string `json:"executable"`
		Console    bool   `json:"console"`
	}{os.Getpid(), executable, console != 0})
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		if scanner.Text() == "stop" {
			os.Exit(23)
		}
	}
	os.Exit(17)
}

type networkTestChild struct {
	command *exec.Cmd
	stdin   io.WriteCloser
	stdout  *bufio.Reader
	stderr  bytes.Buffer
	pid     uint32
}

func startNetworkTestChild(t *testing.T, local, channel string) *networkTestChild {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	t.Cleanup(cancel)
	child := &networkTestChild{}
	child.command = exec.CommandContext(ctx, os.Args[0], "-test.run=^TestNetworkLauncherProcessEntry$")
	child.command.Env = append(os.Environ(), "SHELLFOX_NETWORK_TEST_LOCAL="+local, "SHELLFOX_NETWORK_TEST_CHANNEL="+channel)
	var err error
	child.stdin, err = child.command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := child.command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	child.stdout = bufio.NewReader(stdout)
	child.command.Stderr = &child.stderr
	if err := child.command.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = child.stdin.Close()
		if child.command.ProcessState == nil {
			_ = child.command.Process.Kill()
			_ = child.command.Wait()
		}
	})
	line, err := child.stdout.ReadString('\n')
	if err != nil {
		_ = child.command.Wait()
		t.Fatalf("launcher handshake: %v, stderr: %s", err, child.stderr.String())
	}
	var result struct {
		PID        uint32 `json:"pid"`
		Executable string `json:"executable"`
		Console    bool   `json:"console"`
	}
	if err := json.Unmarshal([]byte(line), &result); err != nil {
		t.Fatal(err)
	}
	want := filepath.Join(local, "Shellfox", "network", "x64", channel, networkRuntimeName)
	if result.PID == 0 || result.Executable != want || result.Console {
		t.Fatalf("unexpected child identity/console: %s", line)
	}
	child.pid = result.PID
	return child
}

func (child *networkTestChild) wait(t *testing.T, want int) {
	t.Helper()
	remaining, err := io.ReadAll(child.stdout)
	if err != nil {
		t.Fatal(err)
	}
	_ = child.command.Wait()
	if child.command.ProcessState.ExitCode() != want || len(remaining) != 0 || child.stderr.Len() != 0 {
		t.Fatalf("launcher exit=%d want=%d stdout=%s stderr=%s", child.command.ProcessState.ExitCode(), want, remaining, child.stderr.String())
	}
}

func TestNetworkRuntimeMultiprocessSameGenerationAndEOFStop(t *testing.T) {
	local := networkTestLocal(t)
	first := startNetworkTestChild(t, local, "")
	second := startNetworkTestChild(t, local, "")
	if first.pid == second.pid {
		t.Fatal("profiles unexpectedly shared a process")
	}
	root := filepath.Join(local, "Shellfox", "network", "x64")
	lock, err := waitNetworkLock(root, time.Second)
	if err != nil {
		t.Fatal("launcher retained deployment lock while waiting", err)
	}
	_ = lock.Close()
	if file, err := stageNetworkExecutable(networkTestSource(t, "different generation"), root); err == nil {
		_ = file.Close()
		t.Fatal("replaced live helper generation")
	}
	_ = first.stdin.Close()
	first.wait(t, 17)
	if _, err := io.WriteString(second.stdin, "stop\n"); err != nil {
		t.Fatal(err)
	}
	second.wait(t, 23)
	file, err := stageNetworkExecutable(networkTestSource(t, "next generation"), root)
	if err != nil {
		t.Fatal("could not replace helper after both profiles exited", err)
	}
	_ = file.Close()
}

func TestNetworkRuntimeChannelsDoNotContend(t *testing.T) {
	local := networkTestLocal(t)
	production := startNetworkTestChild(t, local, "")
	testingChild := startNetworkTestChild(t, local, "testing")
	_ = testingChild.stdin.Close()
	testingChild.wait(t, 17)
	root := filepath.Join(local, "Shellfox", "network", "x64", "testing")
	file, err := stageNetworkExecutable(networkTestSource(t, "new testing generation"), root)
	if err != nil {
		t.Fatal("active production helper blocked testing generation", err)
	}
	_ = file.Close()
	_ = production.stdin.Close()
	production.wait(t, 17)
}

func TestNetworkRuntimeLauncherDeathKillsChild(t *testing.T) {
	child := startNetworkTestChild(t, networkTestLocal(t), "")
	handle, err := windows.OpenProcess(windows.SYNCHRONIZE, false, child.pid)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(handle)
	if err := child.command.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	_ = child.command.Wait()
	// Leave the original stdin writer open: only job lifetime may end the child.
	status, err := windows.WaitForSingleObject(handle, 5000)
	if err != nil || status != windows.WAIT_OBJECT_0 {
		t.Fatalf("launcher orphaned its networking child: status=%d err=%v", status, err)
	}
}
