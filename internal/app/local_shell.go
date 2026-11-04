package app

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

const localShellRelayFrames = 16
const localShellMaxSessions = 32

// localShellState owns every locally spawned shell (Settings → 其他功能 switch).
// Like serial it is independent from the SSH session map: a local tab has no
// SFTP, monitoring, shell integration or window handoff. Windows spawns
// cmd.exe; macOS spawns the user's login shell at the home directory.
type localShellState struct {
	mu       sync.Mutex
	sessions map[string]*localShellSession
	closed   bool
}

type LocalShellInfo struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Shell string `json:"shell"`
	Cwd   string `json:"cwd"`
}

type localShellSession struct {
	app             *App
	info            LocalShellInfo
	pty             *shellPTY
	ctx             context.Context
	cancel          context.CancelFunc
	closeOnce       sync.Once
	mu              sync.Mutex
	terminalStarted bool
}

func (s *localShellSession) Close() {
	s.closeOnce.Do(func() {
		s.cancel()
		// Closing the PTY unblocks the pending relay Read with EOF/EIO, which
		// then observes the cancelled context and exits without an exit note.
		_ = s.pty.Close()
		s.app.removeLocalShellSession(s.info.ID)
	})
}

// localShellCommand resolves the per-platform local shell launch. Windows uses
// cmd.exe (the user's explicit default), macOS uses $SHELL (zsh on modern
// defaults) as a login shell, matching what a native terminal tab opens.
func localShellCommand() (string, []string, string, error) {
	switch runtime.GOOS {
	case "windows":
		command := os.Getenv("ComSpec")
		if command == "" {
			command = "cmd.exe"
		}
		if _, err := os.Stat(command); err != nil {
			command = `C:\Windows\System32\cmd.exe`
			if _, err := os.Stat(command); err != nil {
				return "", nil, "", errors.New("找不到 cmd.exe，无法启动本地终端")
			}
		}
		return command, nil, "命令提示符", nil
	case "darwin":
		shell := os.Getenv("SHELL")
		if shell == "" {
			shell = "/bin/zsh"
		}
		info, err := os.Stat(shell)
		if err != nil || info.IsDir() || info.Mode().Perm()&0o111 == 0 {
			shell = "/bin/zsh"
			if _, err := os.Stat(shell); err != nil {
				shell = "/bin/bash"
				if _, err := os.Stat(shell); err != nil {
					return "", nil, "", errors.New("找不到可用的本地 Shell（$SHELL、/bin/zsh、/bin/bash）")
				}
			}
		}
		return shell, []string{"-l"}, "本地终端 · " + filepath.Base(shell), nil
	default:
		// The released client is Windows/macOS only; the fallback keeps the
		// feature testable on CI hosts without product commitment.
		shell := os.Getenv("SHELL")
		if shell == "" {
			shell = "/bin/sh"
		}
		if _, err := os.Stat(shell); err != nil {
			shell = "/bin/sh"
		}
		return shell, []string{"-l"}, "本地终端", nil
	}
}

func (a *App) localShellSessionByID(id string) (*localShellSession, error) {
	a.local.mu.Lock()
	defer a.local.mu.Unlock()
	s := a.local.sessions[id]
	if s == nil {
		return nil, errors.New("本地终端会话不存在，请重新打开")
	}
	select {
	case <-s.ctx.Done():
		return nil, errors.New("本地终端已关闭，请重新打开")
	default:
		return s, nil
	}
}

func (a *App) removeLocalShellSession(id string) {
	a.local.mu.Lock()
	delete(a.local.sessions, id)
	a.local.mu.Unlock()
}

func (a *App) startLocalShell() (LocalShellInfo, error) {
	if err := a.RequireUnlocked(); err != nil {
		return LocalShellInfo{}, err
	}
	command, args, name, err := localShellCommand()
	if err != nil {
		return LocalShellInfo{}, err
	}
	cwd, err := os.UserHomeDir()
	if err != nil {
		cwd = ""
	}
	pty, err := startShellPTY(command, args, cwd, 100, 30)
	if err != nil {
		return LocalShellInfo{}, fmt.Errorf("启动本地终端失败：%w", err)
	}
	ctx, cancel := context.WithCancel(a.ctx)
	s := &localShellSession{app: a, pty: pty, ctx: ctx, cancel: cancel, info: LocalShellInfo{
		ID: randomID(), Name: name, Shell: command, Cwd: pty.dir,
	}}
	a.local.mu.Lock()
	if a.local.closed || a.ctx.Err() != nil {
		a.local.mu.Unlock()
		cancel()
		_ = pty.Close()
		return LocalShellInfo{}, errors.New("应用正在退出")
	}
	active := 0
	for _, existing := range a.local.sessions {
		if existing.ctx.Err() == nil {
			active++
		}
	}
	if active >= localShellMaxSessions {
		a.local.mu.Unlock()
		cancel()
		_ = pty.Close()
		return LocalShellInfo{}, fmt.Errorf("最多同时打开 %d 个本地终端", localShellMaxSessions)
	}
	if a.local.sessions == nil {
		a.local.sessions = map[string]*localShellSession{}
	}
	a.local.sessions[s.info.ID] = s
	a.local.mu.Unlock()
	// A created session whose terminal never attaches (closed tab, failed
	// renderer) must not keep a shell process running forever.
	time.AfterFunc(45*time.Second, func() {
		s.mu.Lock()
		started := s.terminalStarted
		s.mu.Unlock()
		if !started {
			s.Close()
		}
	})
	return s.info, nil
}

func (a *App) closeLocalShells() {
	a.local.mu.Lock()
	a.local.closed = true
	closing := []*localShellSession{}
	for _, s := range a.local.sessions {
		closing = append(closing, s)
	}
	a.local.mu.Unlock()
	for _, s := range closing {
		s.Close()
	}
}

// localShellTerminal relays the local PTY stream into the same xterm.js
// WebSocket protocol used by SSH and serial terminals. Resize carries real
// PTY geometry, unlike serial.
func (a *App) localShellTerminal(w http.ResponseWriter, r *http.Request) {
	s, err := a.localShellSessionByID(r.PathValue("id"))
	if err != nil {
		writeError(w, 400, err)
		return
	}
	s.mu.Lock()
	if s.terminalStarted {
		s.mu.Unlock()
		writeError(w, 409, errors.New("此本地终端已有终端"))
		return
	}
	s.terminalStarted = true
	s.mu.Unlock()
	upgrader := websocket.Upgrader{CheckOrigin: a.allowedOrigin, HandshakeTimeout: 5 * time.Second, ReadBufferSize: 4096, WriteBufferSize: 32768}
	ws, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		s.mu.Lock()
		s.terminalStarted = false
		s.mu.Unlock()
		return
	}
	ws.SetReadLimit(2 << 20)
	if cols, cerr := strconv.Atoi(r.URL.Query().Get("cols")); cerr == nil && cols >= 2 && cols <= 9999 {
		if rows, rerr := strconv.Atoi(r.URL.Query().Get("rows")); rerr == nil && rows >= 2 && rows <= 9999 {
			_ = s.pty.Resize(cols, rows)
		}
	}
	s.runTerminal(&terminalPeer{ws: ws})
}

func (s *localShellSession) runTerminal(peer *terminalPeer) {
	// Closing the terminal ends the shell process; the tab and the shell share
	// one lifetime, matching the serial behaviour users already know.
	runTerminalStream(s.app, s.ctx, peer, s.pty, "本地 Shell 已退出", s.Close)
}

// terminalStream abstracts the byte pipe behind a terminal relay. The local
// shell's PTY and a telnet session's TCP connection share runTerminalStream;
// Resize is a no-op for streams without geometry.
type terminalStream interface {
	Read(buffer []byte) (int, error)
	Write(data []byte) (int, error)
	Resize(cols, rows int) error
	Wait() error
	Close() error
}

// runTerminalStream relays one terminal WebSocket (the same xterm.js protocol
// as SSH and serial) against an arbitrary stream. The relay closes the session
// through onClose when either side ends, and reports exitNote when the stream
// closes on its own.
func runTerminalStream(app *App, ctx context.Context, peer *terminalPeer, stream terminalStream, exitNote string, onClose func()) {
	defer onClose()
	// A hijacked websocket connection outlives the handler, so close it
	// explicitly when the relay ends — the browser only sees onclose then.
	defer peer.ws.Close()
	write := func(kind int, data []byte) error {
		peer.ws.SetWriteDeadline(time.Now().Add(15 * time.Second))
		return peer.ws.WriteMessage(kind, data)
	}
	status := func(kind, message string) error {
		data, _ := json.Marshal(map[string]string{"type": kind, "message": message})
		return write(websocket.TextMessage, data)
	}
	stopWatch := context.AfterFunc(ctx, func() { peer.ws.Close() })
	defer stopWatch()
	if err := status("ready", ""); err != nil {
		return
	}
	output := make(chan []byte, localShellRelayFrames)
	inputs := make(chan string, 64)
	controls := make(chan relayMessage, 64)
	peerClosed := make(chan struct{})
	readErr := make(chan error, 1)
	go func() {
		defer close(peerClosed)
		for {
			var m relayMessage
			if err := peer.ws.ReadJSON(&m); err != nil {
				return
			}
			select {
			case controls <- m:
			case <-ctx.Done():
				return
			}
		}
	}()
	go func() {
		buffer := make([]byte, 4096)
		for {
			n, err := stream.Read(buffer)
			if n > 0 {
				frame := append([]byte(nil), buffer[:n]...)
				select {
				case output <- frame:
				case <-ctx.Done():
					return
				}
			}
			if err != nil {
				select {
				case readErr <- err:
				case <-ctx.Done():
				}
				return
			}
		}
	}()
	go func() {
		for {
			select {
			case data := <-inputs:
				if app.RequireUnlocked() != nil {
					continue
				}
				if _, err := stream.Write([]byte(data)); err != nil {
					return
				}
			case <-ctx.Done():
				return
			}
		}
	}()
	credits := 0
	for {
		var outputCh chan []byte
		if credits < localShellRelayFrames {
			outputCh = output
		}
		select {
		case <-ctx.Done():
			return
		case <-peerClosed:
			return
		case <-readErr:
			// The PTY closes when the shell exits (EIO/EOF on unix, broken
			// pipe on Windows); a TCP peer closing reads as EOF. Report the
			// exit status when it can be read.
			if ctx.Err() == nil {
				note := exitNote
				if waitErr := stream.Wait(); waitErr != nil {
					note += fmt.Sprintf("（%s）", waitErr)
				}
				_ = status("exit", note)
			}
			return
		case frame := <-outputCh:
			if write(websocket.BinaryMessage, frame) != nil {
				return
			}
			credits++
		case m := <-controls:
			switch m.Type {
			case "ack":
				if credits > 0 {
					credits--
				}
			case "input":
				if app.RequireUnlocked() != nil {
					continue
				}
				if len(m.Data) > 65536 {
					_ = status("error", "输入过多，请分批粘贴")
					return
				}
				select {
				case inputs <- m.Data:
				default:
					_ = status("error", "输入过多，请分批粘贴")
					return
				}
			case "resize":
				if m.Cols >= 2 && m.Rows >= 2 && m.Cols <= 9999 && m.Rows <= 9999 {
					_ = stream.Resize(m.Cols, m.Rows)
				}
			}
		}
	}
}

func (a *App) registerLocalShellHTTP(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/local-shell/sessions", func(w http.ResponseWriter, r *http.Request) {
		info, err := a.startLocalShell()
		respond(w, info, err)
	})
	mux.HandleFunc("GET /api/local-shell/sessions/{id}", func(w http.ResponseWriter, r *http.Request) {
		s, err := a.localShellSessionByID(r.PathValue("id"))
		if err != nil {
			respond(w, nil, err)
			return
		}
		writeJSON(w, s.info)
	})
	mux.HandleFunc("DELETE /api/local-shell/sessions/{id}", func(w http.ResponseWriter, r *http.Request) {
		if s, err := a.localShellSessionByID(r.PathValue("id")); err == nil {
			s.Close()
		}
		writeJSON(w, map[string]bool{"ok": true})
	})
	mux.HandleFunc("GET /api/local-shell/sessions/{id}/terminal", a.localShellTerminal)
}
