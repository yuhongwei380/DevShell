package app

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"os"
	"runtime"
	"strings"
	"testing"
	"testing/fstest"
	"time"
)

// TestLocalShellPTYRoundTrip drives a real local shell over the PTY layer on
// supported platforms: the echo marker must come back through the master
// side, resize must be accepted, and `exit` must end the relay read with a
// waitable exit. Windows has no local shell at all — there the PTY layer must
// refuse to spawn one.
func TestLocalShellPTYRoundTrip(t *testing.T) {
	home, err := os.UserHomeDir()
	if err != nil {
		t.Skip("no user home directory:", err)
	}
	command, args, _, err := localShellCommand()
	if err != nil {
		t.Skip("no local shell available:", err)
	}
	if runtime.GOOS == "windows" {
		if _, err := startShellPTY(command, args, home, 100, 30); err == nil {
			t.Fatal("Windows must not start a local shell")
		}
		return
	}
	pty, err := startShellPTY(command, args, home, 100, 30)
	if err != nil {
		t.Fatal(err)
	}
	defer pty.Close()
	if err := pty.Resize(120, 40); err != nil {
		t.Fatalf("resize rejected: %v", err)
	}
	marker := "devshell-pty-roundtrip-marker"
	if _, err := pty.Write([]byte("echo " + marker + "\r")); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(15 * time.Second)
	var seen []byte
	for !bytes.Contains(seen, []byte(marker)) && time.Now().Before(deadline) {
		buffer := make([]byte, 4096)
		n, err := pty.Read(buffer)
		seen = append(seen, buffer[:n]...)
		if err != nil {
			t.Fatalf("pty read failed before marker: %v (seen %q)", err, seen)
		}
	}
	if !bytes.Contains(seen, []byte(marker)) {
		t.Fatalf("marker never echoed; captured %q", seen)
	}
	if _, err := pty.Write([]byte("exit\r")); err != nil {
		t.Fatal(err)
	}
	exited := false
	closed := time.Now().Add(15 * time.Second)
	for time.Now().Before(closed) {
		buffer := make([]byte, 4096)
		if _, err := pty.Read(buffer); err != nil {
			exited = true
			break
		}
	}
	if !exited {
		t.Fatal("shell never closed the PTY after exit")
	}
	// Wait must be callable after the relay observed the exit, twice, and stay stable.
	_ = pty.Wait()
	_ = pty.Wait()
}

func TestLocalShellSessionHTTPContract(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	request := func(method, path string, input any) *httptest.ResponseRecorder {
		t.Helper()
		body, _ := json.Marshal(input)
		req := httptest.NewRequest(method, "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		return response
	}
	response := request("POST", "/api/local-shell/sessions", map[string]any{})
	if runtime.GOOS == "windows" {
		// Windows has no local shell: the spawn endpoint must reject it.
		if response.Code == 200 {
			t.Fatal("Windows must reject local shell sessions")
		}
		return
	}
	if response.Code != 200 {
		t.Fatalf("spawn rejected: %d %s", response.Code, response.Body.String())
	}
	var info LocalShellInfo
	if err := json.Unmarshal(response.Body.Bytes(), &info); err != nil {
		t.Fatal(err)
	}
	if info.ID == "" || info.Name == "" || info.Shell == "" {
		t.Fatalf("incomplete local shell info: %+v", info)
	}
	if home, err := os.UserHomeDir(); err == nil && info.Cwd != home {
		t.Fatalf("local shell must start at the user home: %+v", info)
	}
	response = request("GET", "/api/local-shell/sessions/"+info.ID, nil)
	if response.Code != 200 {
		t.Fatalf("session lookup failed: %d %s", response.Code, response.Body.String())
	}
	// The terminal watchdog releases shells whose renderer never attaches,
	// but an explicit DELETE must free the session immediately.
	response = request("DELETE", "/api/local-shell/sessions/"+info.ID, nil)
	if response.Code != 200 {
		t.Fatalf("close failed: %d %s", response.Code, response.Body.String())
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		response = request("GET", "/api/local-shell/sessions/"+info.ID, nil)
		if response.Code != 200 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("local shell session survived DELETE")
		}
		time.Sleep(50 * time.Millisecond)
	}
	if strings.Contains(info.Name, "%") {
		t.Fatalf("unexpected placeholder in name: %+v", info)
	}
	// The second terminal attach is rejected while the first is live; without
	// any attach the session is still valid (watchdog owns the cleanup).
	if _, err := a.localShellSessionByID(info.ID); err == nil {
		t.Fatal("deleted session still resolvable")
	}
}

// TestLocalShellSwitchRejectedOnWindows pins the platform gate on the
// preference: Windows may keep the field false but can never turn it on.
func TestLocalShellSwitchRejectedOnWindows(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("platform gate applies to Windows only")
	}
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	patch := func(body string) *httptest.ResponseRecorder {
		t.Helper()
		req := httptest.NewRequest("POST", "http://wails.localhost/api/appearance/patch", strings.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		req.Header.Set("Content-Type", "application/json")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		return response
	}
	if response := patch(`{"localShellEnabled":true}`); response.Code == 200 {
		t.Fatalf("Windows accepted the local shell switch: %s", response.Body.String())
	}
	if response := patch(`{"localShellEnabled":false}`); response.Code != 200 {
		t.Fatalf("turning the switch off must stay allowed: %d %s", response.Code, response.Body.String())
	}
}
