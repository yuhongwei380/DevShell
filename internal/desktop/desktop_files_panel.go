//go:build desktop

package desktop

import "github.com/wailsapp/wails/v2/pkg/runtime"

// SetFilesPanel reserves window space for the right-hand panel. Track only the
// actual growth so closing a panel at the screen edge cannot over-shrink it.
func (d *Desktop) SetFilesPanel(open bool, width int) error {
	d.filesPanelMu.Lock()
	defer d.filesPanelMu.Unlock()
	if d.ctx == nil || d.filesPanelOpen == open {
		return nil
	}
	if runtime.WindowIsMaximised(d.ctx) || runtime.WindowIsFullscreen(d.ctx) {
		d.filesPanelOpen = open
		return nil
	}
	delta := -d.filesPanelGrowth
	if open {
		delta = max(0, min(856, width)) - d.filesPanelGrowth
	}
	growth, err := platformResizeWindowWidth(d.ctx, delta)
	if err != nil {
		return err
	}
	d.filesPanelOpen = open
	if open {
		d.filesPanelGrowth += growth
	} else {
		d.filesPanelGrowth = 0
	}
	return nil
}
