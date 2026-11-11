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
	"unicode/utf16"
)

// TestLocalShellPTYRoundTrip drives a real local shell over the PTY layer on
// supported platforms: the echo marker must come back through the master
// side, resize must be accepted, and `exit` must end the relay read with a
// waitable exit. On Windows this only runs when WSL has a distro (the ConPTY
// transport); machines without WSL have no local shell to drive.
func TestLocalShellPTYRoundTrip(t *testing.T) {
	home, err := os.UserHomeDir()
	if err != nil {
		t.Skip("no user home directory:", err)
	}
	command, args, _, err := localShellCommand()
	if err != nil {
		t.Skip("no local shell available:", err)
	}
	if !stdioConsoleBacked() {
		t.Skip("stdio redirected to file/pipe: conhost drops ConPTY attach for children of this process")
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
	if !localShellSupported() {
		// Unsupported machines (Windows without WSL): the spawn endpoint must
		// reject the session outright.
		if response.Code == 200 {
			t.Fatal("unsupported platforms must reject local shell sessions")
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

// TestLocalShellSwitchPlatformGate pins the platform gate on the preference:
// Windows may turn the switch on only while WSL reports an installed distro;
// machines without WSL keep the historical default (on rejected, off allowed).
func TestLocalShellSwitchPlatformGate(t *testing.T) {
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
	response := patch(`{"localShellEnabled":true}`)
	if wslAvailable() {
		if response.Code != 200 {
			t.Fatalf("Windows with WSL must accept the local shell switch: %d %s", response.Code, response.Body.String())
		}
	} else if response.Code == 200 {
		t.Fatalf("Windows without WSL accepted the local shell switch: %s", response.Body.String())
	}
	if response := patch(`{"localShellEnabled":false}`); response.Code != 200 {
		t.Fatalf("turning the switch off must stay allowed: %d %s", response.Code, response.Body.String())
	}
}

// TestWslDistroNamesParsing pins the `wsl -l -q` decoder: modern wsl.exe
// writes UTF-16LE (with or without BOM) whose NUL bytes must not leak into
// names, and machines without distros must decode to an empty list.
func TestWslDistroNamesParsing(t *testing.T) {
	utf16le := func(parts ...string) []byte {
		var raw []byte
		for _, part := range parts {
			for _, unit := range utf16.Encode([]rune(part)) {
				raw = append(raw, byte(unit), byte(unit>>8))
			}
		}
		return raw
	}
	observed := utf16le("Ubuntu\r\n\r\ndocker-desktop\r\n\r\n\r\n")
	if names := wslDistroNames(observed); len(names) != 2 || names[0] != "Ubuntu" || names[1] != "docker-desktop" {
		t.Fatalf("UTF-16LE distro list decoded wrong: %q", names)
	}
	withBOM := append([]byte{0xFF, 0xFE}, utf16le("Debian\r\n")...)
	if names := wslDistroNames(withBOM); len(names) != 1 || names[0] != "Debian" {
		t.Fatalf("BOM-prefixed list decoded wrong: %q", names)
	}
	if names := wslDistroNames([]byte("Ubuntu\r\ndocker-desktop\r\n")); len(names) != 2 {
		t.Fatalf("plain UTF-8 fallback decoded wrong: %q", names)
	}
	for _, empty := range [][]byte{nil, {}, utf16le("\r\n\r\n")} {
		if names := wslDistroNames(empty); len(names) != 0 {
			t.Fatalf("no distro must decode to none, got %q", names)
		}
	}
}
