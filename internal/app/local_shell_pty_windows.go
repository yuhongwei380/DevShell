//go:build windows

package app

import "errors"

// errLocalShellUnsupported documents the product decision: the local shell
// feature is macOS-only. Windows stays on the start-tab flow, so the pseudo
// console plumbing that used to live here was removed rather than debugged on
// Insider builds whose ConPTY is systemically broken (0xC0000142 / dead pipe
// relay). The type and methods only satisfy the shared session code, which
// never gets a live shellPTY on Windows because startShellPTY always fails.
var errLocalShellUnsupported = errors.New("Windows 暂不支持本地终端")

type shellPTY struct {
	dir string
}

func startShellPTY(command string, args []string, cwd string, cols, rows int) (*shellPTY, error) {
	return nil, errLocalShellUnsupported
}

func (p *shellPTY) Read(buffer []byte) (int, error) { return 0, errLocalShellUnsupported }
func (p *shellPTY) Write(data []byte) (int, error)  { return 0, errLocalShellUnsupported }
func (p *shellPTY) Resize(cols, rows int) error     { return errLocalShellUnsupported }
func (p *shellPTY) Wait() error                     { return errLocalShellUnsupported }
func (p *shellPTY) Close() error                    { return nil }
