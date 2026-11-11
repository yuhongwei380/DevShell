//go:build darwin || linux || freebsd || netbsd || openbsd || dragonfly

package app

import (
	"os"
	"os/exec"
	"sync"

	"github.com/creack/pty"
)

// shellPTY wraps a child process attached to a real Unix PTY, so vim, htop
// and tab completion behave exactly like a native terminal tab.
type shellPTY struct {
	file      *os.File
	cmd       *exec.Cmd
	dir       string
	waitOnce  sync.Once
	waitErr   error
	waitDone  chan struct{}
	closeOnce sync.Once
}

func startShellPTY(command string, args []string, cwd string, cols, rows int) (*shellPTY, error) {
	cmd := exec.Command(command, args...)
	if cwd != "" {
		cmd.Dir = cwd
	}
	env := os.Environ()
	env = append(env, "TERM=xterm-256color")
	cmd.Env = env
	file, err := pty.Start(cmd)
	if err != nil {
		return nil, err
	}
	s := &shellPTY{file: file, cmd: cmd, dir: cmd.Dir, waitDone: make(chan struct{})}
	go func() {
		s.waitErr = cmd.Wait()
		close(s.waitDone)
	}()
	if cols >= 2 && rows >= 2 {
		_ = pty.Setsize(file, &pty.Winsize{Rows: uint16(rows), Cols: uint16(cols)})
	}
	return s, nil
}

func (p *shellPTY) Read(buffer []byte) (int, error)  { return p.file.Read(buffer) }
func (p *shellPTY) Write(data []byte) (int, error)   { return p.file.Write(data) }
func (p *shellPTY) Resize(cols, rows int) error {
	return pty.Setsize(p.file, &pty.Winsize{Rows: uint16(rows), Cols: uint16(cols)})
}

func (p *shellPTY) Wait() error {
	p.waitOnce.Do(func() { <-p.waitDone })
	return p.waitErr
}

// Close terminates the child and releases the PTY. The Wait goroutine still
// reaps the process even when the relay already observed the PTY error.
func (p *shellPTY) Close() error {
	p.closeOnce.Do(func() {
		if p.cmd.Process != nil {
			_ = p.cmd.Process.Kill()
		}
	})
	return p.file.Close()
}

// stdioConsoleBacked mirrors the Windows guard; a Unix PTY does not depend on
// the parent's own stdio, so it is always usable here.
func stdioConsoleBacked() bool { return true }
