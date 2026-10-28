package app

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"golang.org/x/crypto/ssh"
)

func TestSSHDiagnosticPreservesFirstTriggerAndSurvivesRemoval(t *testing.T) {
	a := &App{sessions: map[string]*Session{}}
	a.sshDiagnostics.path = filepath.Join(t.TempDir(), "logs", "ssh-disconnect.jsonl")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	s := &Session{ID: "test-session", ctx: ctx, cancel: cancel, diagnosticLog: &a.sshDiagnostics, diagnosticStarted: time.Now().Add(-2 * time.Minute)}
	a.sessions[s.ID] = s
	s.latency = LatencySample{Pending: true, PendingSince: time.Now().Add(-3 * time.Second)}
	s.forceCloseDiagnostic("DS-230", "ssh-channel-open-cleanup-timeout")
	s.noteDisconnect("DS-202", "local-socket-closed")
	a.disconnect(s.ID)
	r := httptest.NewRequest("GET", "/", nil)
	r.SetPathValue("id", s.ID)
	w := httptest.NewRecorder()
	a.sshDisconnectDiagnosticHTTP(w, r)
	var result SSHDisconnectDiagnostic
	if err := json.Unmarshal(w.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	if result.Code != "DS-230" || result.TraceID != "test-session" || result.AgeSeconds < 120 || result.LogPath == "" {
		t.Fatalf("wrong trigger: %+v", result)
	}
	data, err := os.ReadFile(result.LogPath)
	if err != nil {
		t.Fatal(err)
	}
	var event sshDiagnosticEvent
	if err = json.Unmarshal(data, &event); err != nil {
		t.Fatal(err)
	}
	if !event.HeartbeatPending || event.HeartbeatPendingSeconds < 3 {
		t.Fatalf("missing pre-disconnect heartbeat: %+v", event)
	}
}

func TestSSHDiagnosticsBoundedRotatingLogAndNoRawErrors(t *testing.T) {
	l := &sshDiagnosticLog{path: filepath.Join(t.TempDir(), "ssh-disconnect.jsonl")}
	for i := 0; i < 3; i++ {
		if err := os.WriteFile(l.path, make([]byte, 1<<20), 0600); err != nil {
			t.Fatal(err)
		}
		l.record("one", sshDiagnosticEvent{SSHDisconnectDiagnostic: SSHDisconnectDiagnostic{Code: "DS-202"}, Kind: "disconnect"})
	}
	if _, err := os.Stat(l.path + ".2"); err != nil {
		t.Fatal(err)
	}
	files, err := os.ReadDir(filepath.Dir(l.path))
	if err != nil || len(files) != 3 {
		t.Fatalf("unbounded files: %v %v", files, err)
	}
	for _, f := range files {
		info, _ := f.Info()
		if info.Size() > 1<<20 {
			t.Fatalf("oversized diagnostic file %s", f.Name())
		}
	}
	secret := "not-a-real-password; command-and-private-key-text"
	kind := sshDiagnosticErrorKind(errors.New(secret))
	if strings.Contains(kind, "password") || kind != "transport-or-channel-error" || sshDiagnosticErrorKind(io.EOF) != "eof" {
		t.Fatal("error classification retained untrusted text")
	}
	l.record("two", sshDiagnosticEvent{SSHDisconnectDiagnostic: SSHDisconnectDiagnostic{Code: "DS-202", Detail: kind}, Kind: "disconnect"})
	data, _ := os.ReadFile(l.path)
	if strings.Contains(string(data), secret) {
		t.Fatal("raw error leaked to diagnostics")
	}
}

func TestSSHDiagnosticsNormalExitLeavesNoLogFile(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "logs")
	l := &sshDiagnosticLog{path: filepath.Join(dir, "ssh-disconnect.jsonl")}
	s := &Session{ID: "normal", diagnosticLog: l, diagnosticStarted: time.Now()}
	l.record(s.ID, sshDiagnosticEvent{Kind: "connected"})
	for i := 0; i < 5; i++ {
		l.record(s.ID, sshDiagnosticEvent{Kind: "sample"})
	}
	s.noteDisconnect("DS-100", "remote-shell-exit")
	l.record(s.ID, sshDiagnosticEvent{Kind: "transport-end"})
	l.mu.Lock()
	record := l.records[s.ID]
	l.mu.Unlock()
	if record.Code != "DS-100" || record.LogPath != "" || record.LogWriteFailed {
		t.Fatalf("normal exit must stay in memory without a log path: %+v", record)
	}
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("normal exit wrote a diagnostic log: %v", err)
	}
}

func TestSSHShellExitDiagnosticSeparatesNormalExit(t *testing.T) {
	cases := []struct {
		err  error
		code string
	}{
		{nil, "DS-100"},
		{&ssh.ExitError{}, "DS-100"},
		{fmt.Errorf("wrapped: %w", &ssh.ExitError{}), "DS-100"},
		{io.EOF, "DS-210"},
		{errors.New("connection reset by peer"), "DS-210"},
	}
	for _, c := range cases {
		code, detail := sshShellExitDiagnostic(c.err)
		if code != c.code || detail == "" {
			t.Fatalf("shell exit %v -> %s/%s, want %s", c.err, code, detail, c.code)
		}
	}
}

func TestSSHDiagnosticConcurrentReasonsAndWriteFailure(t *testing.T) {
	l := &sshDiagnosticLog{path: filepath.Join(t.TempDir(), "blocked", "file")}
	if err := os.WriteFile(filepath.Dir(l.path), []byte("not a directory"), 0600); err != nil {
		t.Fatal(err)
	}
	s := &Session{ID: "same", diagnosticLog: l, diagnosticStarted: time.Now()}
	s.noteDisconnect("DS-299", "terminal-relay-ended")
	s.noteDisconnect("DS-201", "heartbeat-timeout")
	var jobs sync.WaitGroup
	for i := 0; i < 20; i++ {
		jobs.Go(func() { s.noteDisconnect("DS-202", "local-socket-closed") })
	}
	jobs.Wait()
	l.mu.Lock()
	record := l.records[s.ID]
	l.mu.Unlock()
	if record.Code != "DS-201" || !record.LogWriteFailed {
		t.Fatalf("lost diagnosis after log write failure: %+v", record)
	}
}

func TestSSHDiagnosticClosePreventsLateLogWrites(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "logs")
	log := &sshDiagnosticLog{path: filepath.Join(dir, "ssh-disconnect.jsonl")}
	log.record("one", sshDiagnosticEvent{SSHDisconnectDiagnostic: SSHDisconnectDiagnostic{Code: "DS-202"}, Kind: "disconnect"})
	log.close()
	if err := os.RemoveAll(dir); err != nil {
		t.Fatal(err)
	}
	var wg sync.WaitGroup
	for i := 0; i < 20; i++ {
		wg.Go(func() { log.record("one", sshDiagnosticEvent{Kind: "transport-end"}) })
	}
	wg.Wait()
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatal("closed diagnostic logger recreated its directory", err)
	}
}

func TestVerboseEventsAreBoundedAndRemainAvailableAfterDisconnect(t *testing.T) {
	a := &App{}
	a.sshDiagnostics.path = filepath.Join(t.TempDir(), "trace.jsonl")
	s := &Session{ID: "trace-session", diagnosticLog: &a.sshDiagnostics, diagnosticStarted: time.Now(), verboseLevel: 3}
	for i := 0; i < 520; i++ {
		s.noteVerbose(2, "verbose-monitor", "system:exec-start")
	}
	s.noteDisconnect("DS-210", "remote-channel-ended-without-status")
	r := httptest.NewRequest("GET", "/?after=518", nil)
	r.SetPathValue("id", s.ID)
	w := httptest.NewRecorder()
	a.sshVerboseHTTP(w, r)
	var result struct {
		Events []sshVerboseEvent `json:"events"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	if len(result.Events) != 3 || result.Events[0].Sequence != 519 || result.Events[2].Kind != "verbose-disconnect" {
		t.Fatalf("unexpected trace: %+v", result.Events)
	}
	a.sshDiagnostics.mu.Lock()
	defer a.sshDiagnostics.mu.Unlock()
	if len(a.sshDiagnostics.verboseEvents[s.ID]) != 512 {
		t.Fatal("trace is not bounded")
	}
}
