//go:build windows

package app

import (
	"bufio"
	"fmt"
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func TestNativeRDPHostChildWindow(t *testing.T) {
	if os.Getenv("DEVSHELL_RDP_HOST_TEST_CHILD") != "1" {
		return
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	class, _ := windows.UTF16PtrFromString("STATIC")
	hwnd, _, err := rdpUser32.NewProc("CreateWindowExW").Call(0, uintptr(unsafe.Pointer(class)), 0, 0, 0, 0, 20, 20, 0, 0, 0, 0)
	if hwnd == 0 {
		t.Fatal(err)
	}
	defer rdpUser32.NewProc("DestroyWindow").Call(hwnd)
	fmt.Fprintln(os.Stdout, hwnd)
	_, _ = bufio.NewReader(os.Stdin).ReadString('\n')
}

func TestNativeRDPHostOnlyAcceptsRegisteredFrontend(t *testing.T) {
	command := exec.Command(os.Args[0], "-test.run=^TestNativeRDPHostChildWindow$")
	command.Env = append(os.Environ(), "DEVSHELL_RDP_HOST_TEST_CHILD=1")
	in, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	out, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err = command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _, _ = in.Write([]byte("\n")); _ = in.Close(); _ = command.Wait() }()
	line, err := bufio.NewReader(out).ReadString('\n')
	if err != nil {
		t.Fatal(err)
	}
	handle, err := strconv.ParseUint(strings.TrimSpace(line), 10, 64)
	if err != nil {
		t.Fatal(err)
	}
	a := &App{}
	if a.validateRDPHost(uintptr(handle)) == nil {
		t.Fatal("unregistered process window accepted")
	}
	a.SetNativeUIProcess(command.Process.Pid)
	if err = a.validateRDPHost(uintptr(handle)); err != nil {
		t.Fatal("registered frontend window rejected:", err)
	}
	a.SetNativeUIProcess(os.Getpid())
	if a.validateRDPHost(uintptr(handle)) == nil {
		t.Fatal("window accepted after replacing the registered process")
	}
	if a.validateRDPHost(0) == nil {
		t.Fatal("null window accepted")
	}
}
