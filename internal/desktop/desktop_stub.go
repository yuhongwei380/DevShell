//go:build !desktop

package desktop

import (
	"devshell/internal/app"
	"errors"
	"io/fs"
)

func Available() bool                                   { return false }
func Run(_ *app.App, _ fs.FS, _ string, _ []byte) error { return nil }

func RunDetached(_ fs.FS, _ []byte) error { return errors.New("独立窗口需要桌面版本") }
