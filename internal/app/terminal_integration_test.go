package app

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/base64"
	"io"
	"os"
	"path"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestTerminalIntegrationTimeoutAndBoundedWorkers(t *testing.T) {
	blocked := make(chan struct{})
	var workers sync.WaitGroup
	var cleaned sync.WaitGroup
	for range cap(terminalIntegrationSlots) {
		workers.Add(1)
		cleaned.Add(1)
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
		result := boundedTerminalIntegration(ctx, func() terminalIntegration { defer workers.Done(); <-blocked; return terminalIntegration{Shell: "bash"} }, func(terminalIntegration) { cleaned.Done() })
		cancel()
		if result.Shell != "" {
			t.Fatal("timed-out integration was enabled")
		}
	}
	start := time.Now()
	result := boundedTerminalIntegration(context.Background(), func() terminalIntegration { t.Error("overflow worker started"); return terminalIntegration{} }, func(terminalIntegration) {})
	if result.Shell != "" || time.Since(start) > time.Second {
		t.Fatal("optional integration blocked terminal fallback")
	}
	close(blocked)
	workers.Wait()
	cleaned.Wait()
}

func TestTerminalIntegrationTemporaryFilesAndRestrictedFallback(t *testing.T) {
	key := os.Getenv("CLOUDSHELL_TEST_KEY")
	if key == "" {
		t.Skip("requires dedicated sshd on 19225")
	}
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	p, err := a.store.Save(Profile{Name: "Shell integration QA", Host: "127.0.0.1", Port: 19225, User: "bitsflow", Group: "QA", Auth: "key", KeyPath: key}, false)
	if err != nil {
		t.Fatal(err)
	}
	s, err := connectLocalSSHFixture(t, a, p.ID, "")
	if err != nil {
		t.Fatal(err)
	}
	defer a.disconnect(s.ID)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	stopSession := context.AfterFunc(ctx, s.forceClose)
	defer stopSession()
	// Exercise the explicit legacy preparation helper independently of the new
	// production inline launch. Staging targets /tmp, never the user home.
	s.startFileInitialization()
	if err := s.waitFileInitialization(ctx); err != nil {
		t.Fatal("fixture SFTP initialization", err)
	}
	files, _ := s.fileClientSnapshot()
	if files == nil {
		t.Fatal("fixture SFTP unavailable")
	}
	integration := s.prepareTerminalIntegrationFiles(ctx, "/tmp")
	if integration.Shell != "bash" || !strings.HasPrefix(integration.directory, "/tmp/dengshell-session-") {
		t.Fatalf("unexpected preparation: %+v", integration)
	}
	defer s.cleanupTerminalIntegration(integration)
	file, err := files.Open(path.Join(integration.directory, "bashrc"))
	if err != nil {
		t.Fatal(err)
	}
	file.Close()
	// An unavailable staging location simply returns no integration metadata.
	fallback := s.prepareTerminalIntegrationFiles(ctx, "/proc")
	if fallback.Shell != "" || fallback.Nonce != "" || fallback.command != "" {
		t.Fatal("restricted staging did not select ordinary shell fallback")
	}
	// Start the real temporary rcfile. It removes all bootstrap files before its
	// ready marker, and does not depend on an eventual clean SSH disconnect.
	sh, err := s.client.NewSession()
	if err != nil {
		t.Fatal(err)
	}
	defer sh.Close()
	if err = sh.RequestPty("xterm-256color", 30, 100, nil); err != nil {
		t.Fatal(err)
	}
	in, err := sh.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	out, err := sh.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err = sh.Start(integration.command); err != nil {
		t.Fatal(err)
	}
	ready := make(chan bool, 1)
	go func() {
		data := make([]byte, 4096)
		text := ""
		for {
			n, err := out.Read(data)
			text += string(data[:n])
			if strings.Contains(text, "DengShell;ready;"+integration.Nonce+";bash") {
				ready <- true
				return
			}
			if err != nil {
				ready <- false
				return
			}
		}
	}()
	select {
	case ok := <-ready:
		if !ok {
			t.Fatal("shell integration readiness missing")
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	if _, err = files.Stat(integration.directory); err == nil {
		t.Fatal("bootstrap directory remained after readiness")
	}
	_, _ = in.Write([]byte("HISTFILE=/dev/null; exit\r"))
	_ = sh.Close()
}

func TestTerminalLaunchPlanDropsOversizedRequest(t *testing.T) {
	// dropbear on OpenWrt 24.10 refused the 26195 byte bootstrap and closed the
	// connection, so the ceiling has to stay below that measurement.
	if terminalLaunchRequestLimit >= 26195 {
		t.Fatalf("limit %d would still send the request dropbear refused", terminalLaunchRequestLimit)
	}
	small := terminalIntegration{Shell: "bash", inline: true, command: posixShellCommand("exec bash -i")}
	if kept, reason := terminalLaunchPlan(small); reason != "" || kept.command == "" || kept.Shell != "bash" {
		t.Fatalf("a fitting plan must be used as built: reason=%q plan=%+v", reason, kept)
	}
	if kept, reason := terminalLaunchPlan(terminalIntegration{}); reason != "" || kept.command != "" {
		t.Fatalf("a plan without a command must stay unchanged: reason=%q plan=%+v", reason, kept)
	}
	oversized := terminalIntegration{Shell: "bash", inline: true, directory: "/tmp/dengshell-session-x", command: posixShellCommand(strings.Repeat("x", terminalLaunchRequestLimit+1))}
	if kept, reason := terminalLaunchPlan(oversized); reason != "too-long" || kept.command != "" || kept.Shell != "" || kept.directory != "" {
		t.Fatalf("oversized plan must fall back to a plain login shell: reason=%q plan=%+v", reason, kept)
	}
}

func TestTerminalLaunchScriptTransportsCompressedBootstrap(t *testing.T) {
	integration := terminalLaunchScript("/tmp", "\n\n")
	if integration.command == "" || !integration.inline {
		t.Fatal("inline launch plan missing")
	}
	if kept, reason := terminalLaunchPlan(integration); reason != "" || len(kept.command) > terminalLaunchRequestLimit {
		t.Fatalf("the production launch request must stay inside the ceiling: reason=%q bytes=%d", reason, len(kept.command))
	}
	// Only the base64 alphabet sits between the quotes, so the payload can be
	// read back without unwinding the shell quoting (the runner's own quoting
	// turns each quote of the payload into the '\'' sequence).
	match := regexp.MustCompile(`printf %s '\\''([A-Za-z0-9+/=]+)`).FindStringSubmatch(integration.command)
	if match == nil {
		t.Fatal("encoded bootstrap payload missing from the runner")
	}
	raw, err := base64.StdEncoding.DecodeString(match[1])
	if err != nil {
		t.Fatalf("payload is not standard base64: %v", err)
	}
	reader, err := gzip.NewReader(bytes.NewReader(raw))
	if err != nil {
		t.Fatalf("payload is not gzip: %v", err)
	}
	script, err := io.ReadAll(reader)
	if err != nil {
		t.Fatal(err)
	}
	text := string(script)
	for _, marker := range []string{`rm -f -- "$0"`, `case "${SHELL##*/}" in`, "bash)", "zsh)", "fish)", "__dengshell_prompt"} {
		if !strings.Contains(text, marker) {
			t.Fatalf("decoded bootstrap lost %q", marker)
		}
	}
	if strings.Contains(integration.command, "__dengshell_prompt") {
		t.Fatal("the plain request still carries bootstrap content")
	}
	launchFile := "/tmp/dengshell-launch-" + integration.Nonce + ".sh"
	cleanup := false
	for _, file := range integration.files {
		if file == launchFile {
			cleanup = true
		}
	}
	if !cleanup {
		t.Fatalf("runner script missing from the cleanup plan: %v", integration.files)
	}
}

type recordingChannelRequester struct {
	mu      sync.Mutex
	names   []string
	replies []bool
}

func (c *recordingChannelRequester) SendRequest(name string, wantReply bool, payload []byte) (bool, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.names = append(c.names, name)
	c.replies = append(c.replies, wantReply)
	return false, nil // dropbear answers unknown channel requests with REQUEST_FAILURE
}

// dropbear -I disconnects despite transport-level keepalive requests, but a
// channel-level request resets the idle timer (reproduced against dropbear
// 2022.83 -I 5: global keepalives every 1s still died at 5.0s with
// "Idle timeout", channel requests survived indefinitely).
func TestDropbearChannelIdleGuardKeepsAskingUntilStopped(t *testing.T) {
	var ch recordingChannelRequester
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() { dropbearChannelIdleGuard(&ch, ctx, 10*time.Millisecond); close(done) }()
	time.Sleep(55 * time.Millisecond)
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("guard kept sending after cancellation")
	}
	ch.mu.Lock()
	defer ch.mu.Unlock()
	if len(ch.names) < 3 {
		t.Fatalf("guard sent %d requests in 55ms at a 10ms interval", len(ch.names))
	}
	for i, name := range ch.names {
		if name != "keepalive@openssh.com" || ch.replies[i] {
			t.Fatalf("request %d = (%q, reply=%v), want (\"keepalive@openssh.com\", false)", i, name, ch.replies[i])
		}
	}
	if dropbearIdleGuardInterval >= 5*time.Second {
		t.Fatalf("interval %v would not stay under the measured -I 5 kill", dropbearIdleGuardInterval)
	}
}
