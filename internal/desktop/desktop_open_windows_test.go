//go:build desktop && windows

package desktop

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"syscall"
	"testing"
	"time"
	"unsafe"

	wailsruntime "github.com/wailsapp/wails/v2/pkg/runtime"
	"golang.org/x/sys/windows"
)

// Model the actual failure condition: the caller's UI thread has hidden the
// pointer after keyboard input. The picker must have an independent, visible
// cursor count without changing the caller's count, on success AND error.
func TestWindowsPickerCursorThreadIsolation(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	showCursor := windowUser32.NewProc("ShowCursor")
	cursorCount := func() int32 {
		showCursor.Call(1)
		count, _, _ := showCursor.Call(0)
		return int32(count)
	}
	for i := 0; i < 3; i++ {
		showCursor.Call(0)
		defer showCursor.Call(1)
	}
	hiddenCount := cursorCount()
	if hiddenCount >= 0 {
		t.Fatalf("测试前提：调用线程应隐藏指针，计数 %d", hiddenCount)
	}
	caller := windows.GetCurrentThreadId()
	failure := errors.New("picker failed")
	for _, expectedError := range []error{nil, failure} {
		value, err := runWindowsFileDialog(func(_ uintptr) (int, error) {
			if windows.GetCurrentThreadId() == caller {
				return 0, errors.New("选择框仍在调用线程上打开")
			}
			if count := cursorCount(); count < 0 {
				return 0, fmt.Errorf("选择框继承隐藏指针计数 %d", count)
			}
			return 42, expectedError
		})
		if value != 42 || !errors.Is(err, expectedError) {
			t.Fatalf("返回值/错误没有透传：%d / %v", value, err)
		}
		if count := cursorCount(); count != hiddenCount {
			t.Fatalf("选择框改变了调用线程的指针计数：%d -> %d", hiddenCount, count)
		}
	}
}

func TestWindowsOpenDialogConfiguration(t *testing.T) {
	for _, kind := range []string{"file", "multi", "folder"} {
		t.Run(kind, func(t *testing.T) {
			directory := t.TempDir()
			_, err := runWindowsFileDialog(func(_ uintptr) (bool, error) {
				dialog, err := newWindowsOpenDialog(wailsruntime.OpenDialogOptions{
					Title: "选择文件", DefaultDirectory: directory, ShowHiddenFiles: true,
					Filters: []wailsruntime.FileFilter{{DisplayName: "字体", Pattern: "*.ttf;*.otf"}},
				}, kind == "multi", kind == "folder")
				if err != nil {
					return false, err
				}
				defer dialog.release()
				var flags uint32
				if err := dialog.call(10, uintptr(unsafe.Pointer(&flags))); err != nil {
					return false, err
				}
				required := uint32(saveForceFilesystem | savePathMustExist | openFileMustExist | openShowHidden)
				if flags&required != required || (flags&openMultiSelect != 0) != (kind == "multi") ||
					(flags&openPickFolders != 0) != (kind == "folder") {
					return false, fmt.Errorf("%s 选项不匹配：%#x", kind, flags)
				}
				if err := dialog.setFilters([]wailsruntime.FileFilter{{DisplayName: "bad\x00name", Pattern: "*.*"}}); err == nil {
					return false, errors.New("非法过滤器必须返回错误")
				}
				return true, nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

// Like the save-picker test, only operate on this test process's unique dialog.
// Cover repeated cancel/accept, Unicode paths, multi-selection and directories.
func TestWindowsOpenDialogNative(t *testing.T) {
	if os.Getenv("DENGSHELL_NATIVE_DIALOG_TEST") != "1" {
		t.Skip("set DENGSHELL_NATIVE_DIALOG_TEST=1 for native picker integration")
	}
	for _, kind := range []string{"file", "multi", "folder"} {
		for _, accept := range []bool{false, true, false, true} {
			t.Run(fmt.Sprintf("%s/accept=%v", kind, accept), func(t *testing.T) {
				directory := t.TempDir()
				names := []string{"测试 file.txt", "另一个.txt"}
				for _, name := range names {
					if err := os.WriteFile(filepath.Join(directory, name), []byte("picker QA"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				title := fmt.Sprintf("DevShell native open QA %d %d", os.Getpid(), time.Now().UnixNano())
				stop := driveTestOpenPicker(title, accept)
				defer stop()
				options := wailsruntime.OpenDialogOptions{Title: title, DefaultDirectory: directory}
				var selected []string
				var err error
				switch kind {
				case "multi":
					options.DefaultFilename = `"` + strings.Join(names, `" "`) + `"`
					selected, err = platformOpenMultipleFilesDialog(context.Background(), options)
				case "file":
					options.DefaultFilename = names[0]
					var path string
					path, err = platformOpenFileDialog(context.Background(), options)
					if path != "" {
						selected = []string{path}
					}
				case "folder":
					var path string
					path, err = platformOpenDirectoryDialog(context.Background(), options)
					if path != "" {
						selected = []string{path}
					}
				}
				if err != nil {
					t.Fatal(err)
				}
				var want []string
				if accept {
					want = []string{filepath.Join(directory, names[0])}
					if kind == "multi" {
						want = append(want, filepath.Join(directory, names[1]))
					} else if kind == "folder" {
						want = []string{directory}
					}
				}
				if !reflect.DeepEqual(selected, want) {
					t.Fatalf("选择结果 %q，期望 %q", selected, want)
				}
			})
		}
	}
}

// Reproduce a keyboard-hidden pointer on the application's main window. Check
// the picker's state at activation and repeatedly during its modal loop without
// moving the user's mouse or changing system pointer settings.
func TestWindowsPickerHiddenOwnerNative(t *testing.T) {
	if os.Getenv("DENGSHELL_NATIVE_DIALOG_TEST") != "1" {
		t.Skip("set DENGSHELL_NATIVE_DIALOG_TEST=1 for native picker integration")
	}
	owner, stopOwner := testHiddenPickerOwner(t)
	defer stopOwner()
	t.Run("error-and-already-disabled", func(t *testing.T) {
		for _, initiallyEnabled := range []uintptr{1, 0} {
			windowUser32.NewProc("EnableWindow").Call(owner, initiallyEnabled)
			failure := errors.New("configuration failed")
			_, err := runWindowsFileDialog(func(_ uintptr) (bool, error) {
				if enabled, _, _ := windowUser32.NewProc("IsWindowEnabled").Call(owner); enabled != 0 {
					return false, errors.New("主窗口未被禁用")
				}
				return false, failure
			})
			enabled, _, _ := windowUser32.NewProc("IsWindowEnabled").Call(owner)
			if !errors.Is(err, failure) || enabled != initiallyEnabled {
				t.Fatalf("异常后窗口状态恢复错误：enabled=%d err=%v", enabled, err)
			}
		}
		windowUser32.NewProc("EnableWindow").Call(owner, 1)
	})
	for _, kind := range []string{"open", "save"} {
		for _, accept := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/accept=%v", kind, accept), func(t *testing.T) {
				checkHiddenOwnerPicker(t, owner, kind, accept)
			})
		}
	}
}

func checkHiddenOwnerPicker(t *testing.T, owner uintptr, kind string, accept bool) {
	t.Helper()
	directory := t.TempDir()
	if err := os.WriteFile(filepath.Join(directory, "source.txt"), []byte("QA"), 0600); err != nil {
		t.Fatal(err)
	}
	title := fmt.Sprintf("DevShell hidden owner QA %d %s %v", os.Getpid(), kind, accept)
	stopPicker := driveTestOpenPickerAfter(title, accept, 2*time.Second)
	defer stopPicker()
	_, err := runWindowsFileDialog(func(actualOwner uintptr) (bool, error) {
		if actualOwner != 0 {
			return false, fmt.Errorf("选择框仍关联 WebView 窗口 %x", actualOwner)
		}
		checked := false
		var dialogWindow uintptr
		var checkError error
		checks := 0
		timerCallback := syscall.NewCallback(func(_ uintptr, _ uint32, _ uintptr, _ uint32) uintptr {
			if !checked {
				return 0
			}
			checks++
			// Model later WebView input/paint events while the shell pumps messages.
			windowUser32.NewProc("SendMessageW").Call(owner, 0x8001, 0, 0)
			defer windowUser32.NewProc("SendMessageW").Call(owner, 0x8002, 0, 0)
			arrow, _, _ := windowUser32.NewProc("LoadCursorW").Call(0, 32512)
			previous, _, _ := windowUser32.NewProc("SetCursor").Call(arrow)
			defer windowUser32.NewProc("SetCursor").Call(previous)
			oldOwnerCursor, _, _ := windowUser32.NewProc("SendMessageW").Call(owner, 0x8003, 0, 0)
			defer windowUser32.NewProc("SendMessageW").Call(owner, 0x8004, oldOwnerCursor, 0)
			cursor, _, _ := windowUser32.NewProc("GetCursor").Call()
			windowUser32.NewProc("ShowCursor").Call(1)
			count, _, _ := windowUser32.NewProc("ShowCursor").Call(0)
			if cursor != arrow || int32(count) < 0 {
				checkError = fmt.Errorf("弹窗运行期间指针受主窗口影响：cursor=%x count=%d", cursor, int32(count))
			}
			// Exercise the shell's normal mouse cursor dispatch as well.
			windowUser32.NewProc("SetCursor").Call(0)
			windowUser32.NewProc("SendMessageW").Call(dialogWindow, 0x20, dialogWindow, 1|(0x200<<16)) // WM_SETCURSOR, HTCLIENT, WM_MOUSEMOVE
			if cursor, _, _ := windowUser32.NewProc("GetCursor").Call(); cursor == 0 {
				checkError = errors.New("选择框处理鼠标移动后未恢复指针句柄")
			}
			return 0
		})
		timer, _, timerError := windowUser32.NewProc("SetTimer").Call(0, 0, 30, timerCallback)
		if timer == 0 {
			return false, timerError
		}
		defer windowUser32.NewProc("KillTimer").Call(0, timer)
		callback := syscall.NewCallback(func(code int32, window, data uintptr) uintptr {
			// Let the production modality hook run before checking dialog state.
			result, _, _ := windowUser32.NewProc("CallNextHookEx").Call(0, uintptr(code), window, data)
			if code == 5 { // HCBT_ACTIVATE
				parent, _, _ := windowUser32.NewProc("GetWindow").Call(window, 4) // GW_OWNER
				if parent == 0 {
					checked = true
					dialogWindow = window
					// Reproduce Chromium hiding again AFTER the activation guard ran.
					windowUser32.NewProc("SendMessageW").Call(owner, 0x8001, 0, 0)
					defer windowUser32.NewProc("SendMessageW").Call(owner, 0x8002, 0, 0)
					ownerThread, _, _ := windowUser32.NewProc("GetWindowThreadProcessId").Call(owner, 0)
					enabled, _, _ := windowUser32.NewProc("IsWindowEnabled").Call(owner)
					windowUser32.NewProc("ShowCursor").Call(1)
					count, _, _ := windowUser32.NewProc("ShowCursor").Call(0)
					if enabled != 0 || uint32(ownerThread) == windows.GetCurrentThreadId() || int32(count) < 0 {
						checkError = fmt.Errorf("模态窗口未隔离或指针仍隐藏：cursor=%d", int32(count))
					}
				}
			}
			return result
		})
		hook, _, err := windowUser32.NewProc("SetWindowsHookExW").Call(5, callback, 0, uintptr(windows.GetCurrentThreadId()))
		if hook == 0 {
			return false, err
		}
		defer windowUser32.NewProc("UnhookWindowsHookEx").Call(hook)
		var selected bool
		if kind == "save" {
			var path string
			path, err = showWindowsSaveDialog(actualOwner, wailsruntime.SaveDialogOptions{
				Title: title, DefaultDirectory: directory, DefaultFilename: "download.txt",
			})
			selected = path != ""
		} else {
			var paths []string
			paths, err = showWindowsOpenDialog(actualOwner, wailsruntime.OpenDialogOptions{
				Title: title, DefaultDirectory: directory, DefaultFilename: "source.txt",
			}, true, false)
			selected = len(paths) != 0
		}
		if err != nil {
			return false, err
		}
		if !checked {
			return false, errors.New("没有检查到模态选择框激活")
		}
		if checks < 3 {
			return false, fmt.Errorf("弹窗运行期间检查不足：%d", checks)
		}
		if selected != accept {
			return false, fmt.Errorf("选择状态 %v，期望 %v", selected, accept)
		}
		return true, checkError
	})
	if err != nil {
		t.Fatal(err)
	}
	if enabled, _, _ := windowUser32.NewProc("IsWindowEnabled").Call(owner); enabled == 0 {
		t.Fatal("关闭选择框后主窗口仍被禁用")
	}
	count, _, _ := windowUser32.NewProc("SendMessageW").Call(owner, 0x8000, 0, 0)
	if int32(count) != -3 {
		t.Fatalf("选择框未平衡指针计数，父窗口剩余 %d，期望 -3", int32(count))
	}
}

func testHiddenPickerOwner(t *testing.T) (uintptr, func()) {
	t.Helper()
	type result struct {
		handle uintptr
		err    error
	}
	ready := make(chan result, 1)
	stopped := make(chan struct{})
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		defer close(stopped)
		className, _ := windows.UTF16PtrFromString("DevShellWindow")
		module, _, _ := windowKernel32.NewProc("GetModuleHandleW").Call(0)
		type windowClass struct {
			size, style          uint32
			proc                 uintptr
			classExtra, winExtra int32
			instance, icon       uintptr
			cursor, background   uintptr
			menu, name           *uint16
			smallIcon            uintptr
		}
		class := windowClass{instance: module, name: className}
		class.size = uint32(unsafe.Sizeof(class))
		class.proc = syscall.NewCallback(func(hwnd uintptr, msg uint32, wparam, lparam uintptr) uintptr {
			if msg == 0x8003 || msg == 0x8004 {
				cursor, _, _ := windowUser32.NewProc("SetCursor").Call(wparam)
				return cursor
			}
			if msg == 0x8001 || msg == 0x8002 {
				show := uintptr(0)
				if msg == 0x8002 {
					show = 1
				}
				count, _, _ := windowUser32.NewProc("ShowCursor").Call(show)
				return count
			}
			if msg == 0x8000 { // query this thread's cursor count without changing it
				windowUser32.NewProc("ShowCursor").Call(1)
				count, _, _ := windowUser32.NewProc("ShowCursor").Call(0)
				return count
			}
			if msg == 2 { // WM_DESTROY
				windowUser32.NewProc("PostQuitMessage").Call(0)
			}
			result, _, _ := windowUser32.NewProc("DefWindowProcW").Call(hwnd, uintptr(msg), wparam, lparam)
			return result
		})
		atom, _, err := windowUser32.NewProc("RegisterClassExW").Call(uintptr(unsafe.Pointer(&class)))
		if atom == 0 {
			ready <- result{err: err}
			return
		}
		defer windowUser32.NewProc("UnregisterClassW").Call(uintptr(unsafe.Pointer(className)), module)
		hwnd, _, err := windowUser32.NewProc("CreateWindowExW").Call(0, uintptr(unsafe.Pointer(className)), 0, 0xcf0000,
			0, 0, 300, 200, 0, 0, module, 0)
		if hwnd == 0 {
			ready <- result{err: err}
			return
		}
		for i := 0; i < 3; i++ {
			windowUser32.NewProc("ShowCursor").Call(0)
			defer windowUser32.NewProc("ShowCursor").Call(1)
		}
		ready <- result{handle: hwnd}
		var message struct {
			hwnd           uintptr
			kind           uint32
			wparam, lparam uintptr
			time           uint32
			x, y           int32
			private        uint32
		}
		for {
			got, _, _ := windowUser32.NewProc("GetMessageW").Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
			if int32(got) <= 0 {
				return
			}
			windowUser32.NewProc("TranslateMessage").Call(uintptr(unsafe.Pointer(&message)))
			windowUser32.NewProc("DispatchMessageW").Call(uintptr(unsafe.Pointer(&message)))
		}
	}()
	r := <-ready
	if r.err != nil {
		t.Fatal(r.err)
	}
	return r.handle, func() {
		windowUser32.NewProc("PostMessageW").Call(r.handle, 0x10, 0, 0) // WM_CLOSE
		select {
		case <-stopped:
		case <-time.After(5 * time.Second):
			t.Error("测试所属窗口未关闭")
		}
	}
}

func driveTestOpenPicker(title string, accept bool) func() {
	return driveTestOpenPickerAfter(title, accept, 250*time.Millisecond)
}

func driveTestOpenPickerAfter(title string, accept bool, delay time.Duration) func() {
	done := make(chan struct{})
	go func() {
		caption, _ := windows.UTF16PtrFromString(title)
		deadline := time.Now().Add(10 * time.Second)
		var foundAt time.Time
		for {
			select {
			case <-done:
				return
			case <-time.After(100 * time.Millisecond):
			}
			handle, _, _ := windowUser32.NewProc("FindWindowW").Call(0, uintptr(unsafe.Pointer(caption)))
			if handle == 0 {
				continue
			}
			var processID uint32
			windowUser32.NewProc("GetWindowThreadProcessId").Call(handle, uintptr(unsafe.Pointer(&processID)))
			if processID != uint32(os.Getpid()) {
				continue
			}
			if foundAt.IsZero() {
				foundAt = time.Now()
			}
			if time.Since(foundAt) < delay {
				continue
			}
			command := uintptr(2) // IDCANCEL
			if accept && time.Now().Before(deadline) {
				command = 1 // IDOK
			}
			windowUser32.NewProc("PostMessageW").Call(handle, 0x111, command, 0)
		}
	}()
	return func() { close(done) }
}
