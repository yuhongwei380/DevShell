package app

import (
	"context"
	"encoding/json"
	"net/http/httptest"
	"testing"

	"golang.org/x/crypto/ssh"
)

func TestDropbearDisablesSFTPWithoutOpeningChannel(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	// The embedded nil Conn panics if a channel is opened, so this also checks
	// that repeated initialization never contacts the server.
	s := &Session{ID: "dropbear", ctx: ctx, client: &ssh.Client{Conn: backgroundVersionConn{version: "SSH-2.0-dropbear"}}}
	s.prepareFileInitialization()
	if !s.SFTPDisabled || s.SFTPPending || s.SFTPAvailable {
		t.Fatal("Dropbear must report SFTP disabled immediately")
	}
	s.startFileInitialization()
	s.startFileInitialization()
	if err := s.waitFileInitialization(ctx); err != nil {
		t.Fatal(err)
	}
	a := &App{sessions: map[string]*Session{s.ID: s}}
	if _, err := a.fileSession(s.ID); err != errSFTPDisabled {
		t.Fatalf("wrong file error: %v", err)
	}
	req := httptest.NewRequest("GET", "/file-status", nil)
	req.SetPathValue("id", s.ID)
	w := httptest.NewRecorder()
	a.fileStatus(w, req)
	var status struct {
		Disabled  bool `json:"sftpDisabled"`
		Available bool `json:"sftpAvailable"`
		Pending   bool `json:"sftpPending"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &status); err != nil || w.Code != 200 || !status.Disabled || status.Available || status.Pending {
		t.Fatalf("incorrect status: %d %s (%v)", w.Code, w.Body.String(), err)
	}
}
