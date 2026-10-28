package app

import (
	"context"
	"errors"
	"testing"
	"time"

	"golang.org/x/crypto/ssh"
)

type backgroundVersionConn struct {
	ssh.Conn
	version string
}

func (c backgroundVersionConn) ServerVersion() []byte { return []byte(c.version) }

func TestDropbearBackgroundGateSerializesAndHonorsCancellation(t *testing.T) {
	s := &Session{client: &ssh.Client{Conn: backgroundVersionConn{version: "SSH-2.0-dropbear"}}}
	release, err := s.acquireDropbearBackground(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	if _, err = s.acquireDropbearBackground(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("waiting gate: %v", err)
	}
	ready := make(chan func(), 1)
	go func() {
		next, err := s.acquireDropbearBackground(context.Background())
		if err == nil {
			ready <- next
		}
	}()
	select {
	case <-ready:
		t.Fatal("concurrent operation entered gate")
	case <-time.After(20 * time.Millisecond):
	}
	release()
	select {
	case next := <-ready:
		next()
	case <-time.After(time.Second):
		t.Fatal("gate not released")
	}
}

func TestNonDropbearBackgroundDoesNotSerialize(t *testing.T) {
	s := &Session{client: &ssh.Client{Conn: backgroundVersionConn{version: "SSH-2.0-OpenSSH"}}}
	first, err := s.acquireDropbearBackground(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer first()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	second, err := s.acquireDropbearBackground(ctx)
	if err != nil {
		t.Fatal(err)
	}
	second()
}
