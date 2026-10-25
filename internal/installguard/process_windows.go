package installguard

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	user32         = windows.NewLazySystemDLL("user32.dll")
	restartManager = windows.NewLazySystemDLL("rstrtmgr.dll")
)

type uniqueProcess struct {
	PID     uint32
	Started windows.Filetime
}
type processInfo struct {
	Process               uniqueProcess
	Name                  [256]uint16
	Service               [64]uint16
	Type, Status, Session uint32
	Restartable           int32
}
type resourceSession struct {
	handle uint32
	paths  []string
}
type runningProcess struct {
	pid     uint32
	started windows.Filetime
	path    string
}

type processWindows struct {
	pid        uint32
	main, tray uintptr
}

var enumerateProcessWindows = windows.NewCallback(func(hwnd, data uintptr) uintptr {
	state := (*processWindows)(unsafe.Pointer(data))
	var pid uint32
	user32.NewProc("GetWindowThreadProcessId").Call(hwnd, uintptr(unsafe.Pointer(&pid)))
	if pid != state.pid {
		return 1
	}
	var class [128]uint16
	user32.NewProc("GetClassNameW").Call(hwnd, uintptr(unsafe.Pointer(&class[0])), uintptr(len(class)))
	switch windows.UTF16ToString(class[:]) {
	case "DevShellWindow", "DengShellWindow":
		state.main = hwnd
	case "DevShellTrayWindow", "DengShellTrayWindow":
		state.tray = hwnd
	}
	return 1
})

func newResourceSession(directory string) (*resourceSession, error) {
	session := &resourceSession{}
	var key [33]uint16
	status, _, _ := restartManager.NewProc("RmStartSession").Call(uintptr(unsafe.Pointer(&session.handle)), 0, uintptr(unsafe.Pointer(&key[0])))
	if status != 0 {
		return nil, windows.Errno(status)
	}
	for _, name := range []string{"DevShell.exe", "DengShell.exe"} {
		path := filepath.Join(directory, name)
		if _, err := os.Stat(path); err == nil {
			session.paths = append(session.paths, path)
		} else if !os.IsNotExist(err) {
			session.close()
			return nil, err
		}
	}
	if len(session.paths) == 0 {
		return session, nil
	}
	pointers := make([]*uint16, len(session.paths))
	for i, path := range session.paths {
		pointers[i], _ = windows.UTF16PtrFromString(path)
	}
	status, _, _ = restartManager.NewProc("RmRegisterResources").Call(uintptr(session.handle), uintptr(len(pointers)), uintptr(unsafe.Pointer(&pointers[0])), 0, 0, 0, 0)
	runtime.KeepAlive(pointers)
	if status != 0 {
		session.close()
		return nil, windows.Errno(status)
	}
	return session, nil
}

func (s *resourceSession) close() { restartManager.NewProc("RmEndSession").Call(uintptr(s.handle)) }

func (s *resourceSession) processes() ([]runningProcess, error) {
	if len(s.paths) == 0 {
		return nil, nil
	}
	var count, needed, reboot uint32
	var info []processInfo
	for attempt := 0; attempt < 8; attempt++ {
		var pointer uintptr
		if len(info) > 0 {
			pointer = uintptr(unsafe.Pointer(&info[0]))
		}
		status, _, _ := restartManager.NewProc("RmGetList").Call(uintptr(s.handle), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&count)), pointer, uintptr(unsafe.Pointer(&reboot)))
		if status == uintptr(windows.ERROR_MORE_DATA) {
			count = needed
			info = make([]processInfo, count)
			continue
		}
		if status != 0 {
			return nil, fmt.Errorf("RmGetList：%w", windows.Errno(status))
		}
		var result []runningProcess
		for _, item := range info[:count] {
			// Telemetry/scanners may briefly read the image without blocking
			// replacement. Never stop Windows services or unrelated applications;
			// the checked rename/rollback path handles any real remaining lock.
			if item.Type == 3 || item.Type == 1000 {
				continue
			} // RmService / RmCritical
			if item.Status&2 != 0 && item.Status&1 == 0 {
				continue
			} // RmStatusStopped
			handle, err := openProcess(item.Process, windows.PROCESS_QUERY_LIMITED_INFORMATION)
			if errors.Is(err, windows.ERROR_INVALID_PARAMETER) || errors.Is(err, os.ErrProcessDone) {
				continue
			}
			if err != nil {
				return nil, fmt.Errorf("无法检查占用程序 %s（PID %d）：%w", windows.UTF16ToString(item.Name[:]), item.Process.PID, err)
			}
			var path [32768]uint16
			length := uint32(len(path))
			err = windows.QueryFullProcessImageName(handle, 0, &path[0], &length)
			state, _ := windows.WaitForSingleObject(handle, 0)
			windows.CloseHandle(handle)
			if state == windows.WAIT_OBJECT_0 {
				continue
			}
			if err != nil {
				return nil, fmt.Errorf("读取占用进程 %d 路径：%w", item.Process.PID, err)
			}
			actual := windows.UTF16ToString(path[:length])
			if resolved, err := filepath.EvalSymlinks(actual); err == nil {
				actual = resolved
			}
			owned := false
			for _, expected := range s.paths {
				if strings.EqualFold(actual, expected) {
					owned = true
					break
				}
			}
			if !owned {
				continue
			}
			result = append(result, runningProcess{pid: item.Process.PID, started: item.Process.Started, path: actual})
		}
		return result, nil
	}
	return nil, errors.New("占用程序频繁变化，请稍后安装")
}

// Compare process creation time on an open handle so a reused PID can never
// receive a forced termination intended for a previous DevShell instance.
func openProcess(process uniqueProcess, access uint32) (windows.Handle, error) {
	handle, err := windows.OpenProcess(access|windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.SYNCHRONIZE, false, process.PID)
	if err != nil {
		return 0, err
	}
	if state, _ := windows.WaitForSingleObject(handle, 0); state == windows.WAIT_OBJECT_0 {
		windows.CloseHandle(handle)
		return 0, os.ErrProcessDone
	}
	var created, exited, kernel, user windows.Filetime
	if err = windows.GetProcessTimes(handle, &created, &exited, &kernel, &user); err != nil {
		windows.CloseHandle(handle)
		return 0, err
	}
	if created != process.Started {
		windows.CloseHandle(handle)
		return 0, os.ErrProcessDone
	}
	return handle, nil
}

func quitMessage() uintptr {
	name, _ := windows.UTF16PtrFromString(QuitMessage)
	message, _, _ := user32.NewProc("RegisterWindowMessageW").Call(uintptr(unsafe.Pointer(name)))
	return message
}

func (p runningProcess) requestClose(cancel bool) bool {
	handle, err := openProcess(uniqueProcess{p.pid, p.started}, windows.SYNCHRONIZE)
	if err != nil {
		return false
	}
	defer windows.CloseHandle(handle)
	message := quitMessage()
	request := uintptr(QuitRequest)
	if cancel {
		request = QuitCancel
	}
	state := processWindows{pid: p.pid}
	user32.NewProc("EnumWindows").Call(enumerateProcessWindows, uintptr(unsafe.Pointer(&state)))
	if state.tray != 0 && message != 0 {
		var response uintptr
		ok, _, _ := user32.NewProc("SendMessageTimeoutW").Call(state.tray, message, request, 0, 2, 1000, uintptr(unsafe.Pointer(&response)))
		if ok != 0 && response == QuitAcknowledged {
			return true
		}
	}
	// Older releases do not understand the installer message. Their regular
	// close path presents existing edit/transfer confirmation, including tray.
	if !cancel && state.main != 0 {
		ok, _, _ := user32.NewProc("PostMessageW").Call(state.main, 0x10, 0, 0)
		return ok != 0
	}
	return false
}

func (p runningProcess) terminate() error {
	handle, err := openProcess(uniqueProcess{p.pid, p.started}, windows.PROCESS_TERMINATE|windows.SYNCHRONIZE)
	if errors.Is(err, windows.ERROR_INVALID_PARAMETER) || errors.Is(err, os.ErrProcessDone) {
		return nil
	}
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	if err = windows.TerminateProcess(handle, 1); err != nil {
		return err
	}
	status, err := windows.WaitForSingleObject(handle, 5000)
	if err == nil && status != windows.WAIT_OBJECT_0 {
		return fmt.Errorf("等待程序退出超时")
	}
	return err
}
