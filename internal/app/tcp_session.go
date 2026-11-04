package app

// Telnet sessions dial an arbitrary host:port over TCP and relay the raw
// stream into a terminal tab through the same WebSocket protocol as SSH,
// serial and local shell tabs. Windows ships no telnet client and macOS marks
// it deprecated, so the app speaks TCP itself; candidate-bar "telnet 主机
// [端口]" (default 23) opens one in the current tab. Windows and macOS are
// both supported. Gated by Settings → 其他功能 (tcpToolsEnabled, default on),
// which also gates the tcping probe endpoint.

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

const tcpSessionMaxSessions = 32

type tcpSessionState struct {
	mu       sync.Mutex
	sessions map[string]*tcpSession
	closed   bool
}

type TcpSessionInfo struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	Host   string `json:"host"`
	Port   int    `json:"port"`
	Remote string `json:"remote"`
}

type tcpSession struct {
	app             *App
	info            TcpSessionInfo
	stream          *telnetStream
	ctx             context.Context
	cancel          context.CancelFunc
	closeOnce       sync.Once
	mu              sync.Mutex
	terminalStarted bool
}

func (s *tcpSession) Close() {
	s.closeOnce.Do(func() {
		s.cancel()
		// Closing the socket unblocks the relay Read with EOF, which then
		// observes the cancelled context and exits without an exit note.
		_ = s.stream.Close()
		s.app.removeTcpSession(s.info.ID)
	})
}

func (a *App) tcpSessionEnabled() error {
	if a.RequireUnlocked() != nil {
		return a.RequireUnlocked()
	}
	if !a.Appearance().TcpToolsEnabled {
		return errors.New("tcping 与 telnet 已关闭，请在「设置 → 其他功能」开启")
	}
	return nil
}

func tcpTargetFrom(input struct {
	Host string `json:"host"`
	Port int    `json:"port"`
}) (string, int, error) {
	host := strings.Trim(strings.TrimSpace(input.Host), "[]")
	if !validQuickHost(host) {
		return "", 0, errors.New("只接受主机名或 IP")
	}
	port := input.Port
	if port == 0 {
		port = 23
	}
	if port < 1 || port > 65535 {
		return "", 0, errors.New("端口应为 1–65535")
	}
	return host, port, nil
}

func (a *App) tcpSessionByID(id string) (*tcpSession, error) {
	a.tcp.mu.Lock()
	defer a.tcp.mu.Unlock()
	s := a.tcp.sessions[id]
	if s == nil {
		return nil, errors.New("TCP 会话不存在，请重新打开")
	}
	select {
	case <-s.ctx.Done():
		return nil, errors.New("TCP 会话已关闭，请重新打开")
	default:
		return s, nil
	}
}

func (a *App) removeTcpSession(id string) {
	a.tcp.mu.Lock()
	delete(a.tcp.sessions, id)
	a.tcp.mu.Unlock()
}

func (a *App) startTcpSession(input struct {
	Host string `json:"host"`
	Port int    `json:"port"`
}) (TcpSessionInfo, error) {
	if err := a.tcpSessionEnabled(); err != nil {
		return TcpSessionInfo{}, err
	}
	host, port, err := tcpTargetFrom(input)
	if err != nil {
		return TcpSessionInfo{}, err
	}
	dialer := net.Dialer{Timeout: 6 * time.Second}
	conn, err := dialer.DialContext(a.ctx, "tcp", net.JoinHostPort(host, strconv.Itoa(port)))
	if err != nil {
		return TcpSessionInfo{}, fmt.Errorf("连接 %s:%d 失败：%s", host, port, tcpFailureNote(err))
	}
	ctx, cancel := context.WithCancel(a.ctx)
	remote := conn.RemoteAddr().String()
	s := &tcpSession{app: a, stream: &telnetStream{conn: conn}, ctx: ctx, cancel: cancel, info: TcpSessionInfo{
		ID: randomID(), Name: fmt.Sprintf("telnet %s:%d", host, port), Host: host, Port: port, Remote: remote,
	}}
	a.tcp.mu.Lock()
	if a.tcp.closed || a.ctx.Err() != nil {
		a.tcp.mu.Unlock()
		cancel()
		_ = conn.Close()
		return TcpSessionInfo{}, errors.New("应用正在退出")
	}
	active := 0
	for _, existing := range a.tcp.sessions {
		if existing.ctx.Err() == nil {
			active++
		}
	}
	if active >= tcpSessionMaxSessions {
		a.tcp.mu.Unlock()
		cancel()
		_ = conn.Close()
		return TcpSessionInfo{}, fmt.Errorf("最多同时打开 %d 个 TCP 会话", tcpSessionMaxSessions)
	}
	if a.tcp.sessions == nil {
		a.tcp.sessions = map[string]*tcpSession{}
	}
	a.tcp.sessions[s.info.ID] = s
	a.tcp.mu.Unlock()
	// A created session whose terminal never attaches (closed tab, failed
	// renderer) must not keep the remote connection open forever.
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

func (a *App) closeTcpSessions() {
	a.tcp.mu.Lock()
	a.tcp.closed = true
	closing := []*tcpSession{}
	for _, s := range a.tcp.sessions {
		closing = append(closing, s)
	}
	a.tcp.mu.Unlock()
	for _, s := range closing {
		s.Close()
	}
}

// tcpTerminal relays a telnet session over the shared terminal protocol.
// Columns and rows carry no meaning for a raw TCP stream, so resize is a no-op.
func (a *App) tcpTerminal(w http.ResponseWriter, r *http.Request) {
	s, err := a.tcpSessionByID(r.PathValue("id"))
	if err != nil {
		writeError(w, 400, err)
		return
	}
	s.mu.Lock()
	if s.terminalStarted {
		s.mu.Unlock()
		writeError(w, 409, errors.New("此 TCP 会话已有终端"))
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
	runTerminalStream(s.app, s.ctx, &terminalPeer{ws: ws}, s.stream, "远端已关闭连接", s.Close)
}

func (a *App) registerTcpSessionHTTP(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/tcp-sessions", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Host string `json:"host"`
			Port int    `json:"port"`
		}
		if !decode(w, r, &input) {
			return
		}
		info, err := a.startTcpSession(input)
		respond(w, info, err)
	})
	mux.HandleFunc("GET /api/tcp-sessions/{id}", func(w http.ResponseWriter, r *http.Request) {
		s, err := a.tcpSessionByID(r.PathValue("id"))
		if err != nil {
			respond(w, nil, err)
			return
		}
		writeJSON(w, s.info)
	})
	mux.HandleFunc("DELETE /api/tcp-sessions/{id}", func(w http.ResponseWriter, r *http.Request) {
		if s, err := a.tcpSessionByID(r.PathValue("id")); err == nil {
			s.Close()
		}
		writeJSON(w, map[string]bool{"ok": true})
	})
	mux.HandleFunc("GET /api/tcp-sessions/{id}/terminal", a.tcpTerminal)
}

// telnetStream wraps a TCP connection and answers telnet negotiation locally:
// WILL/DO offers are refused with WONT/DON'T and never reach the screen, sub-
// negotiation blocks are dropped, and the 0xFF 0xFF escape stays literal so
// binary payloads survive. This keeps raw port checks clean and plain telnet
// servers usable without a full option stack.
const (
	telnetIAC  = 0xFF
	telnetSE   = 240
	telnetSB   = 250
	telnetWONT = 252
	telnetWILL = 251
	telnetDONT = 254
	telnetDO   = 253
)

type telnetStream struct {
	conn       net.Conn
	pending    []byte
	readErr    error
	inCommand  bool
	inSubneg   bool
	waitingOpt byte
}

func (t *telnetStream) Read(p []byte) (int, error) {
	for len(t.pending) == 0 {
		if t.readErr != nil {
			return 0, t.readErr
		}
		raw := make([]byte, 4096)
		n, err := t.conn.Read(raw)
		if n > 0 {
			t.feed(raw[:n])
		}
		if err != nil && t.readErr == nil {
			t.readErr = err
		}
		if len(t.pending) == 0 && err != nil {
			return 0, err
		}
	}
	n := copy(p, t.pending)
	t.pending = t.pending[n:]
	return n, nil
}

func (t *telnetStream) feed(raw []byte) {
	for _, b := range raw {
		switch {
		case t.inSubneg:
			if t.inCommand {
				if b == telnetSE {
					t.inSubneg = false
				}
				t.inCommand = false
			} else if b == telnetIAC {
				t.inCommand = true
			}
		case t.waitingOpt != 0:
			switch t.waitingOpt {
			case telnetWILL:
				t.negotiate(telnetWONT, b)
			case telnetDO:
				t.negotiate(telnetDONT, b)
			}
			t.waitingOpt = 0
		case t.inCommand:
			t.inCommand = false
			switch b {
			case telnetIAC:
				t.pending = append(t.pending, telnetIAC)
			case telnetWILL, telnetDO:
				t.waitingOpt = b
			case telnetSB:
				t.inSubneg = true
			}
		case b == telnetIAC:
			t.inCommand = true
		default:
			t.pending = append(t.pending, b)
		}
	}
}

func (t *telnetStream) negotiate(command, option byte) {
	_, _ = t.conn.Write([]byte{telnetIAC, command, option})
}

func (t *telnetStream) Write(data []byte) (int, error) { return t.conn.Write(data) }
func (t *telnetStream) Resize(cols, rows int) error    { return nil }
func (t *telnetStream) Wait() error                    { return nil }
func (t *telnetStream) Close() error                   { return t.conn.Close() }

// tcpFailureNote maps dial errors to the short Chinese notes shown in the
// terminal (the raw Go wording embeds addresses and syscall noise, and the
// refused/DNS phrasings differ between unix and Windows).
func tcpFailureNote(err error) string {
	text := strings.ToLower(err.Error())
	switch {
	case strings.Contains(text, "refused"): // unix "connection refused", Windows "actively refused"
		return "端口拒绝连接"
	case errors.Is(err, context.DeadlineExceeded) || strings.Contains(text, "timeout"):
		return "连接超时"
	case strings.Contains(text, "no such host"):
		return "域名解析失败"
	case errors.Is(err, context.Canceled):
		return "已取消"
	default:
		return err.Error()
	}
}
