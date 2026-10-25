//go:build desktop && windows

package desktop

import (
	"context"
	"fmt"
	"unsafe"
)

func platformResizeWindowWidth(_ context.Context, delta int) (int, error) {
	if delta == 0 {
		return 0, nil
	}
	handle := platformWindowHandle()
	var rect desktopRect
	ok, _, err := windowUser32.NewProc("GetWindowRect").Call(handle, uintptr(unsafe.Pointer(&rect)))
	if ok == 0 {
		return 0, fmt.Errorf("无法读取窗口大小: %w", err)
	}
	monitor, _, _ := windowUser32.NewProc("MonitorFromWindow").Call(handle, 2)
	info := desktopMonitorInfo{}
	info.Size = uint32(unsafe.Sizeof(info))
	ok, _, err = windowUser32.NewProc("GetMonitorInfoW").Call(monitor, uintptr(unsafe.Pointer(&info)))
	if ok == 0 {
		return 0, fmt.Errorf("无法读取窗口工作区: %w", err)
	}
	dpi := uintptr(96)
	getDPI := windowUser32.NewProc("GetDpiForWindow")
	if getDPI.Find() == nil {
		if value, _, _ := getDPI.Call(handle); value != 0 {
			dpi = value
		}
	}
	oldWidth := int(rect.Right - rect.Left)
	workWidth := int(info.Work.Right - info.Work.Left)
	width := min(workWidth, max(min(workWidth, defaultMinWindowWidth*int(dpi)/96), oldWidth+delta*int(dpi)/96))
	x := max(int(info.Work.Left), min(int(rect.Left), int(info.Work.Right)-width))
	ok, _, err = windowUser32.NewProc("SetWindowPos").Call(handle, 0, uintptr(x), uintptr(rect.Top), uintptr(width), uintptr(rect.Bottom-rect.Top), 0x0014)
	if ok == 0 {
		return 0, fmt.Errorf("无法调整窗口宽度: %w", err)
	}
	return (width - oldWidth) * 96 / int(dpi), nil
}
