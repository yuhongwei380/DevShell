//go:build desktop && !windows

package desktop

import (
	"context"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

func platformResizeWindowWidth(ctx context.Context, delta int) (int, error) {
	width, height := runtime.WindowGetSize(ctx)
	limit := width + max(0, delta)
	screens, err := runtime.ScreenGetAll(ctx)
	if err != nil {
		return 0, err
	}
	for _, screen := range screens {
		if screen.IsCurrent {
			limit = screen.Size.Width
			break
		}
	}
	next := min(limit, max(min(defaultMinWindowWidth, limit), width+delta))
	runtime.WindowSetSize(ctx, next, height)
	return next - width, nil
}
