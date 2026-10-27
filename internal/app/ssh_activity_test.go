package app

import (
	"bytes"
	"context"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// -vvv level 3 packet logging must land on disk while the transport is still
// alive: an idle-timeout kill must not take the timeline down with it.
func TestVerboseActivityConnPacketsReachDisk(t *testing.T) {
	log := &sshDiagnosticLog{path: filepath.Join(t.TempDir(), "ssh-disconnect.jsonl")}
	server, client := net.Pipe()
	defer server.Close()
	activity := newSSHActivityConn(client)
	s := &Session{ID: "verbose-activity", ctx: context.Background(), diagnosticLog: log, verboseLevel: 3, diagnosticStarted: time.Now()}
	activity.enableVerbose(s)
	go func() {
		buf := make([]byte, 64)
		for {
			if _, err := server.Read(buf); err != nil {
				return
			}
		}
	}()
	if _, err := activity.Write([]byte("hello")); err != nil {
		t.Fatal(err)
	}
	go func() {
		if _, err := server.Write([]byte("reply")); err != nil {
			t.Log(err)
		}
	}()
	buf := make([]byte, 16)
	if _, err := activity.Read(buf); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(2 * time.Second)
	var data []byte
	for time.Now().Before(deadline) {
		data, _ = os.ReadFile(log.path)
		if bytes.Contains(data, []byte(`"write=1 bytes=5"`)) && bytes.Contains(data, []byte(`"read=1 bytes=5`)) {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("verbose packet events never reached disk: %s", data)
}
