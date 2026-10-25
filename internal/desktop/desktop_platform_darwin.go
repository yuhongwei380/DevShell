//go:build desktop && darwin

package desktop

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa -framework UniformTypeIdentifiers
#import <Cocoa/Cocoa.h>

typedef struct { int width, height, workWidth, workHeight; } DevShellScreen;
static DevShellScreen dengshell_main_screen(void) {
	@autoreleasepool {
		NSScreen *screen = [NSScreen mainScreen];
		NSRect frame = [screen frame], work = [screen visibleFrame];
		return (DevShellScreen){(int)frame.size.width, (int)frame.size.height,
			(int)work.size.width, (int)work.size.height};
	}
}
*/
import "C"

import (
	"context"

	"github.com/wailsapp/wails/v2/pkg/runtime"
)

func initialPlatformWindow() (initialWindowBounds, error) {
	screen := C.dengshell_main_screen()
	return initialWindowForMonitor(monitorBounds{
		Width: int(screen.width), Height: int(screen.height),
		WorkWidth: int(screen.workWidth), WorkHeight: int(screen.workHeight),
	}), nil
}

func finalizeInitialPlatformWindow(ctx context.Context, _ initialWindowBounds) {
	runtime.WindowCenter(ctx)
}

func platformRuntimeDescription() string { return "macOS Cocoa / system WebKit" }
