package main

import (
	"os"
	"runtime"
	"unsafe"

	"golang.org/x/sys/windows"
)

// PROC_THREAD_ATTRIBUTE_JOB_LIST, available on supported Windows 10+ systems.
// Assignment during creation avoids an orphan window between Start and Assign.
const networkJobListAttribute = 0x0002000d

type networkProcess struct {
	process windows.Handle
	job     windows.Handle
}

func startNetworkProcess(executable string, args []string, stdin, stdout, stderr *os.File) (*networkProcess, error) {
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, err
	}
	child := &networkProcess{job: job}
	ok := false
	defer func() {
		if !ok {
			child.close()
		}
	}()
	limits := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limits.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limits)), uint32(unsafe.Sizeof(limits))); err != nil {
		return nil, err
	}
	attributes, err := windows.NewProcThreadAttributeList(2)
	if err != nil {
		return nil, err
	}
	defer attributes.Delete()
	if err := attributes.Update(networkJobListAttribute, unsafe.Pointer(&job), unsafe.Sizeof(job)); err != nil {
		return nil, err
	}
	var handles []windows.Handle
	defer func() { closeNetworkHandles(handles) }()
	for _, file := range []*os.File{stdin, stdout, stderr} {
		var handle windows.Handle
		if err := windows.DuplicateHandle(windows.CurrentProcess(), windows.Handle(file.Fd()), windows.CurrentProcess(), &handle, 0, true, windows.DUPLICATE_SAME_ACCESS); err != nil {
			return nil, err
		}
		handles = append(handles, handle)
	}
	if err := attributes.Update(windows.PROC_THREAD_ATTRIBUTE_HANDLE_LIST, unsafe.Pointer(&handles[0]), uintptr(len(handles))*unsafe.Sizeof(handles[0])); err != nil {
		return nil, err
	}
	startup := windows.StartupInfoEx{
		StartupInfo: windows.StartupInfo{
			Cb: uint32(unsafe.Sizeof(windows.StartupInfoEx{})), Flags: windows.STARTF_USESTDHANDLES | windows.STARTF_USESHOWWINDOW,
			ShowWindow: windows.SW_HIDE, StdInput: handles[0], StdOutput: handles[1], StdErr: handles[2],
		},
		ProcThreadAttributeList: attributes.List(),
	}
	application, err := windows.UTF16PtrFromString(executable)
	if err != nil {
		return nil, err
	}
	commandLine, err := windows.UTF16PtrFromString(windows.ComposeCommandLine(append([]string{executable}, args...)))
	if err != nil {
		return nil, err
	}
	var info windows.ProcessInformation
	err = windows.CreateProcess(application, commandLine, nil, nil, true,
		windows.CREATE_NO_WINDOW|windows.CREATE_UNICODE_ENVIRONMENT|windows.EXTENDED_STARTUPINFO_PRESENT,
		nil, nil, &startup.StartupInfo, &info)
	runtime.KeepAlive(handles)
	runtime.KeepAlive(job)
	if err != nil {
		return nil, err
	}
	_ = windows.CloseHandle(info.Thread)
	child.process = info.Process
	ok = true
	return child, nil
}

func (child *networkProcess) wait() (int, error) {
	if _, err := windows.WaitForSingleObject(child.process, windows.INFINITE); err != nil {
		return 1, err
	}
	var code uint32
	if err := windows.GetExitCodeProcess(child.process, &code); err != nil {
		return 1, err
	}
	return int(code), nil
}

func (child *networkProcess) close() {
	if child.job != 0 {
		_ = windows.CloseHandle(child.job)
		child.job = 0
	}
	if child.process != 0 {
		_ = windows.CloseHandle(child.process)
		child.process = 0
	}
}
