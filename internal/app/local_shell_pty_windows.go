//go:build windows

package app

import (
	"errors"
	"fmt"
	"strings"
	"sync"
	"unsafe"

	"golang.org/x/sys/windows"
)

// ConPTY-backed PTY for the local shell. Windows local tabs run WSL
// (wsl.exe), which behaves correctly under a pseudo console but not over
// plain pipes, so the session rides CreatePseudoConsole the same way a
// Windows Terminal profile does. The attribute-list and CreateProcessW
// plumbing stays raw because golang.org/x/sys does not export the
// extended-startup-info entry points.

var (
	modkernel32                       = windows.NewLazySystemDLL("kernel32.dll")
	procInitializeProcThreadAttribute = modkernel32.NewProc("InitializeProcThreadAttributeList")
	procUpdateProcThreadAttribute     = modkernel32.NewProc("UpdateProcThreadAttribute")
	procDeleteProcThreadAttribute     = modkernel32.NewProc("DeleteProcThreadAttributeList")
	procCreateProcessW                = modkernel32.NewProc("CreateProcessW")
)

type shellPTY struct {
	dir         string
	console     windows.Handle
	inWrite     windows.Handle // parent side of the conpty input pipe
	outRead     windows.Handle // parent side of the conpty output pipe
	process     windows.Handle
	attrList    []byte // must outlive the attach: conhost reads it after CreateProcessW returns
	closeOnce   sync.Once
	consoleOnce sync.Once
}

func startShellPTY(command string, args []string, cwd string, cols, rows int) (*shellPTY, error) {
	if cols < 2 || rows < 2 {
		cols, rows = 100, 30
	}
	// The ConPTY contract requires inheritable pipe ends.
	inheritable := windows.SecurityAttributes{InheritHandle: 1}
	var inputRead, inputWrite, outputRead, outputWrite windows.Handle
	if err := windows.CreatePipe(&inputRead, &inputWrite, &inheritable, 0); err != nil {
		return nil, fmt.Errorf("创建输入管道失败：%w", err)
	}
	if err := windows.CreatePipe(&outputRead, &outputWrite, &inheritable, 0); err != nil {
		windows.CloseHandle(inputRead)
		windows.CloseHandle(inputWrite)
		return nil, fmt.Errorf("创建输出管道失败：%w", err)
	}
	var console windows.Handle
	if err := windows.CreatePseudoConsole(windows.Coord{X: int16(cols), Y: int16(rows)}, inputRead, outputWrite, 0, &console); err != nil {
		for _, handle := range []windows.Handle{inputRead, inputWrite, outputRead, outputWrite} {
			windows.CloseHandle(handle)
		}
		return nil, fmt.Errorf("创建伪控制台失败：%w", err)
	}
	var attrList []byte
	fail := func(err error) (*shellPTY, error) {
		if attrList != nil {
			procDeleteProcThreadAttribute.Call(uintptr(unsafe.Pointer(&attrList[0])))
		}
		windows.ClosePseudoConsole(console)
		for _, handle := range []windows.Handle{inputRead, inputWrite, outputRead, outputWrite} {
			windows.CloseHandle(handle)
		}
		return nil, err
	}

	// First call only sizes the buffer and is expected to report failure.
	var attributeSize uintptr
	procInitializeProcThreadAttribute.Call(0, 1, 0, uintptr(unsafe.Pointer(&attributeSize)))
	if attributeSize == 0 {
		return fail(errors.New("初始化进程属性列表失败：系统未返回所需大小"))
	}
	attrList = make([]byte, attributeSize)
	if ret, _, errno := procInitializeProcThreadAttribute.Call(uintptr(unsafe.Pointer(&attrList[0])), 1, 0, uintptr(unsafe.Pointer(&attributeSize))); ret == 0 {
		return fail(fmt.Errorf("初始化进程属性列表失败：%w", errno))
	}
	// PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE carries the handle value itself.
	if ret, _, errno := procUpdateProcThreadAttribute.Call(uintptr(unsafe.Pointer(&attrList[0])), 0, windows.PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, uintptr(console), unsafe.Sizeof(console), 0, 0); ret == 0 {
		return fail(fmt.Errorf("绑定伪控制台失败：%w", errno))
	}

	commandLine := `"` + command + `"`
	if len(args) > 0 {
		commandLine += " " + strings.Join(args, " ")
	}
	commandLineRaw, err := windows.UTF16PtrFromString(commandLine)
	if err != nil {
		return fail(fmt.Errorf("命令行无效：%w", err))
	}
	var cwdRaw *uint16
	if cwd != "" {
		if cwdRaw, err = windows.UTF16PtrFromString(cwd); err != nil {
			return fail(fmt.Errorf("工作目录无效：%w", err))
		}
	}
	var startup windows.StartupInfoEx
	startup.Cb = uint32(unsafe.Sizeof(startup))
	startup.ProcThreadAttributeList = (*windows.ProcThreadAttributeList)(unsafe.Pointer(&attrList[0]))
	var processInfo windows.ProcessInformation
	ret, _, errno := procCreateProcessW.Call(0, uintptr(unsafe.Pointer(commandLineRaw)), 0, 0, 0,
		uintptr(windows.EXTENDED_STARTUPINFO_PRESENT), 0, uintptr(unsafe.Pointer(cwdRaw)),
		uintptr(unsafe.Pointer(&startup)), uintptr(unsafe.Pointer(&processInfo)))
	if ret == 0 {
		return fail(fmt.Errorf("启动 %s 失败：%w", command, errno))
	}
	windows.CloseHandle(processInfo.Thread)
	// The parent does not use the conpty-side pipe ends; the parent only keeps
	// its own write/read handles.
	windows.CloseHandle(inputRead)
	windows.CloseHandle(outputWrite)
	pty := &shellPTY{dir: cwd, console: console, inWrite: inputWrite, outRead: outputRead, process: processInfo.Process, attrList: attrList}
	// Unlike a unix pty, a conpty does NOT close its pipes when the attached
	// process tree exits — readers would block forever. Watch the process and
	// tear the pseudo console down, mirroring what Windows Terminal does.
	go func() {
		windows.WaitForSingleObject(pty.process, windows.INFINITE)
		pty.closeConsole()
	}()
	return pty, nil
}

func (p *shellPTY) Read(buffer []byte) (int, error) {
	var done uint32
	err := windows.ReadFile(p.outRead, buffer, &done, nil)
	return int(done), err
}

func (p *shellPTY) Write(data []byte) (int, error) {
	var done uint32
	err := windows.WriteFile(p.inWrite, data, &done, nil)
	return int(done), err
}

func (p *shellPTY) Resize(cols, rows int) error {
	return windows.ResizePseudoConsole(p.console, windows.Coord{X: int16(cols), Y: int16(rows)})
}

func (p *shellPTY) Wait() error {
	windows.WaitForSingleObject(p.process, windows.INFINITE)
	return nil
}

func (p *shellPTY) closeConsole() {
	p.consoleOnce.Do(func() {
		// Closing the pseudo console tears down the whole attached process
		// tree and unblocks pending Reads with ERROR_BROKEN_PIPE, mirroring
		// the unix SIGHUP/EIO path.
		windows.ClosePseudoConsole(p.console)
	})
}

func (p *shellPTY) Close() error {
	p.closeOnce.Do(func() {
		p.closeConsole()
		windows.CloseHandle(p.outRead)
		windows.CloseHandle(p.inWrite)
		windows.CloseHandle(p.process)
		if p.attrList != nil {
			// The attribute list backing the startup info is released with
			// the session rather than right after CreateProcessW so the
			// pseudoconsole reference stays pinned for the whole attach.
			procDeleteProcThreadAttribute.Call(uintptr(unsafe.Pointer(&p.attrList[0])))
			p.attrList = nil
		}
	})
	return nil
}

// stdioConsoleBacked reports whether this process' stderr is a console (or
// absent, the desktop launch case). When stderr is a file or pipe, this
// Insider build's conhost silently drops pseudoconsole attach for children
// spawned by the redirected process — real DevShell (GUI subsystem, no stdio
// handles, or console windows) never hits that, but `go test` relays its
// child stdio over pipes, so the PTY round-trip test must not run there.
func stdioConsoleBacked() bool {
	handle, err := windows.GetStdHandle(windows.STD_ERROR_HANDLE)
	if err != nil || handle == 0 || handle == windows.InvalidHandle {
		return true
	}
	mode, _ := windows.GetFileType(handle)
	switch mode {
	case windows.FILE_TYPE_DISK, windows.FILE_TYPE_PIPE:
		return false
	}
	return true
}
