package app

import (
	"encoding/json"
	"net"
	"strconv"
	"strings"
	"testing"
	"time"
)

// listenThenFree returns a loopback port with no listener attached, so dialing
// it is rejected immediately (no firewall timeouts).
func listenThenFree(t *testing.T) int {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	_ = listener.Close()
	time.Sleep(50 * time.Millisecond)
	return port
}

func TestTCPingProbeContract(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	// Accepting listener: every attempt reports a timed handshake and the
	// summary counts all of them.
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			_ = conn.Close()
		}
	}()
	port := listener.Addr().(*net.TCPAddr).Port
	w := issueRequest(t, a, "POST", "/api/local-probe/tcping", map[string]any{"host": "127.0.0.1", "port": port, "count": 2})
	if w.Code != 200 {
		t.Fatalf("tcping on an open port = %d: %s", w.Code, w.Body.String())
	}
	payload := decodeProbe(t, w)
	output, _ := payload["output"].(string)
	for _, want := range []string{"本机 TCP → 127.0.0.1", "已连接", "成功 2 / 2 次连接"} {
		if !strings.Contains(output, want) {
			t.Fatalf("open-port output %q must contain %q", output, want)
		}
	}
	if payload["ok"] != true {
		t.Fatalf("open port must report ok: %v", payload)
	}
	// Closed port: attempts fail with the short Chinese note, ok is false.
	closed := listenThenFree(t)
	w = issueRequest(t, a, "POST", "/api/local-probe/tcping", map[string]any{"host": "127.0.0.1", "port": closed, "count": 1})
	payload = decodeProbe(t, w)
	output, _ = payload["output"].(string)
	if !strings.Contains(output, "连接失败 · 端口拒绝连接") || payload["ok"] != false {
		t.Fatalf("closed-port output = %q ok=%v", output, payload["ok"])
	}
	// brief mode (frontend -t loop) drops the header and the summary line.
	w = issueRequest(t, a, "POST", "/api/local-probe/tcping", map[string]any{"host": "127.0.0.1", "port": port, "count": 1, "brief": true})
	payload = decodeProbe(t, w)
	output, _ = payload["output"].(string)
	if strings.Contains(output, "本机 TCP") || strings.Contains(output, "成功 ") {
		t.Fatalf("brief output must stay a single line: %q", output)
	}
	// Invalid targets are rejected without dialing.
	for _, body := range []map[string]any{
		{"host": "bad host;rm", "port": 80},
		{"host": "127.0.0.1", "port": 0},
		{"host": "127.0.0.1", "port": 70000},
		{"host": "127.0.0.1"},
	} {
		if w := issueRequest(t, a, "POST", "/api/local-probe/tcping", body); w.Code != 400 {
			t.Fatalf("tcping %v = %d, want 400", body, w.Code)
		}
	}
}

func TestTCPSessionContract(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	accepted := make(chan struct{}, 4)
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			accepted <- struct{}{}
			go func() { time.Sleep(2 * time.Second); _ = conn.Close() }()
		}
	}()
	port := listener.Addr().(*net.TCPAddr).Port
	w := issueRequest(t, a, "POST", "/api/tcp-sessions", map[string]any{"host": "127.0.0.1", "port": port})
	if w.Code != 200 {
		t.Fatalf("dial local listener = %d: %s", w.Code, w.Body.String())
	}
	var info TcpSessionInfo
	if err := json.Unmarshal(w.Body.Bytes(), &info); err != nil {
		t.Fatal(err)
	}
	if info.ID == "" || info.Name != "telnet 127.0.0.1:"+strconv.Itoa(port) || info.Host != "127.0.0.1" || info.Port != port {
		t.Fatalf("session info = %+v", info)
	}
	select {
	case <-accepted:
	case <-time.After(3 * time.Second):
		t.Fatal("the listener never saw the dial")
	}
	if w := issueRequest(t, a, "GET", "/api/tcp-sessions/"+info.ID, nil); w.Code != 200 {
		t.Fatalf("GET live session = %d: %s", w.Code, w.Body.String())
	}
	if w := issueRequest(t, a, "DELETE", "/api/tcp-sessions/"+info.ID, nil); w.Code != 200 {
		t.Fatalf("DELETE = %d: %s", w.Code, w.Body.String())
	}
	if w := issueRequest(t, a, "GET", "/api/tcp-sessions/"+info.ID, nil); w.Code != 400 {
		t.Fatalf("GET closed session = %d, want 400", w.Code)
	}
	// Unreachable port: creation fails with the short note, no session left.
	closed := listenThenFree(t)
	if w := issueRequest(t, a, "POST", "/api/tcp-sessions", map[string]any{"host": "127.0.0.1", "port": closed}); w.Code != 400 || !strings.Contains(w.Body.String(), "端口拒绝连接") {
		t.Fatalf("dial closed port = %d: %s", w.Code, w.Body.String())
	}
	// Default port is 23 when omitted, and junk targets are rejected.
	if w := issueRequest(t, a, "POST", "/api/tcp-sessions", map[string]any{"host": "127.0.0.1", "port": 0}); w.Code == 200 {
		t.Fatal("port 0 must not silently dial port 23 through the API")
	}
	for _, body := range []map[string]any{{"host": "bad host;rm"}, {"host": "127.0.0.1", "port": 999999}} {
		if w := issueRequest(t, a, "POST", "/api/tcp-sessions", body); w.Code != 400 {
			t.Fatalf("tcp-sessions %v = %d, want 400", body, w.Code)
		}
	}
}

func TestTCPToolsSwitchGatesBothEndpoints(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	if _, err := a.store.PatchAppearance(map[string]json.RawMessage{"tcpToolsEnabled": json.RawMessage("false")}); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	port := listener.Addr().(*net.TCPAddr).Port
	if w := issueRequest(t, a, "POST", "/api/local-probe/tcping", map[string]any{"host": "127.0.0.1", "port": port}); w.Code != 400 || !strings.Contains(w.Body.String(), "其他功能") {
		t.Fatalf("tcping with the switch off = %d: %s", w.Code, w.Body.String())
	}
	if w := issueRequest(t, a, "POST", "/api/tcp-sessions", map[string]any{"host": "127.0.0.1", "port": port}); w.Code != 400 || !strings.Contains(w.Body.String(), "其他功能") {
		t.Fatalf("telnet with the switch off = %d: %s", w.Code, w.Body.String())
	}
	if _, err := a.store.PatchAppearance(map[string]json.RawMessage{"tcpToolsEnabled": json.RawMessage("true")}); err != nil {
		t.Fatal(err)
	}
	if w := issueRequest(t, a, "POST", "/api/local-probe/tcping", map[string]any{"host": "127.0.0.1", "port": port, "count": 1}); w.Code != 200 {
		t.Fatalf("tcping after re-enabling = %d: %s", w.Code, w.Body.String())
	}
}

// fakeTCPConn feeds canned inbound bytes and records outbound writes, so the
// telnet negotiation filter is testable without a socket pair.
type fakeTCPConn struct {
	in      []byte
	pos     int
	written []byte
	closed  bool
}

func (c *fakeTCPConn) Read(p []byte) (int, error) {
	if c.pos >= len(c.in) {
		return 0, net.ErrClosed
	}
	n := copy(p, c.in[c.pos:])
	c.pos += n
	return n, nil
}
func (c *fakeTCPConn) Write(p []byte) (int, error) {
	c.written = append(c.written, p...)
	return len(p), nil
}
func (c *fakeTCPConn) Close() error                     { c.closed = true; return nil }
func (c *fakeTCPConn) LocalAddr() net.Addr              { return &net.TCPAddr{} }
func (c *fakeTCPConn) RemoteAddr() net.Addr             { return &net.TCPAddr{} }
func (c *fakeTCPConn) SetDeadline(time.Time) error      { return nil }
func (c *fakeTCPConn) SetReadDeadline(time.Time) error  { return nil }
func (c *fakeTCPConn) SetWriteDeadline(time.Time) error { return nil }

func TestTelnetNegotiationFilter(t *testing.T) {
	conn := &fakeTCPConn{in: []byte{
		telnetIAC, telnetWILL, 1, // WILL ECHO → refused with WONT
		telnetIAC, telnetDO, 24, // DO TERMINAL-TYPE → refused with DON'T
		'h', 'i',
		telnetIAC, telnetIAC, // escaped 0xFF stays literal
		telnetIAC, telnetSB, 0, telnetIAC, telnetSE, // sub-negotiation dropped
		'!',
	}}
	stream := &telnetStream{conn: conn}
	var got []byte
	buffer := make([]byte, 64)
	for {
		n, err := stream.Read(buffer)
		got = append(got, buffer[:n]...)
		if err != nil {
			break
		}
	}
	want := []byte{'h', 'i', 0xFF, '!'}
	if string(got) != string(want) {
		t.Fatalf("filtered stream = %v, want %v", got, want)
	}
	wantReplies := []byte{telnetIAC, telnetWONT, 1, telnetIAC, telnetDONT, 24}
	if string(conn.written) != string(wantReplies) {
		t.Fatalf("negotiation replies = %v, want %v", conn.written, wantReplies)
	}
}
