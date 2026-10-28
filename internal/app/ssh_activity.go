package app

import (
	"fmt"
	"net"
	"sync/atomic"
	"time"
)

var sshActivityEpoch = time.Now()

// Observe inbound transport traffic, including terminal and SFTP packets.
// Successful local writes alone are not proof of delivery to the peer.
type sshActivityConn struct {
	net.Conn
	receivedAt atomic.Int64
	verbose    atomic.Pointer[Session] // set only when -vvv packet activity logging is on
	readCount  atomic.Int32
	writeCount atomic.Int32
}

func newSSHActivityConn(conn net.Conn) *sshActivityConn {
	c := &sshActivityConn{Conn: conn}
	c.receivedAt.Store(time.Since(sshActivityEpoch).Nanoseconds())
	return c
}

// enableVerbose starts packet activity logging. The store is the publication
// point for the session pointer: Connect sets every field the logging path
// touches before this call, so transport goroutines never see a partially
// initialised session.
func (c *sshActivityConn) enableVerbose(s *Session) { c.verbose.Store(s) }

func (c *sshActivityConn) Read(data []byte) (int, error) {
	n, err := c.Conn.Read(data)
	idle := c.idleFor()
	if n > 0 {
		c.receivedAt.Store(time.Since(sshActivityEpoch).Nanoseconds())
		if s := c.verbose.Load(); s != nil {
			c.note(s, "read", n, idle)
		}
	}
	return n, err
}

func (c *sshActivityConn) Write(data []byte) (int, error) {
	n, err := c.Conn.Write(data)
	if n > 0 {
		if s := c.verbose.Load(); s != nil {
			c.note(s, "write", n, 0)
		}
	}
	return n, err
}

const (
	// sshVerbosePacketEvents caps raw packet logging per direction; after it
	// only the gaps that matter are still recorded.
	sshVerbosePacketEvents = 200
	// Once capped, inbound packets arriving after this much silence are still
	// recorded: those gaps bracket the server-side idle-timeout kills that
	// -vvv is usually invoked to catch.
	sshVerboseIdleGap = 10 * time.Second
)

func (c *sshActivityConn) note(s *Session, direction string, n int, idle time.Duration) {
	if direction == "read" {
		switch count := c.readCount.Add(1); {
		case count <= sshVerbosePacketEvents:
			s.noteVerbose(3, "verbose-packet", fmt.Sprintf("read=%d bytes=%d idle=%.2f", count, n, idle.Seconds()))
		case count == sshVerbosePacketEvents+1:
			s.noteVerbose(3, "verbose-packets-capped", fmt.Sprintf("limit=%d", sshVerbosePacketEvents))
		case idle >= sshVerboseIdleGap:
			s.noteVerbose(3, "verbose-read-gap", fmt.Sprintf("bytes=%d idle=%.2f", n, idle.Seconds()))
		}
		return
	}
	if count := c.writeCount.Add(1); count <= sshVerbosePacketEvents {
		s.noteVerbose(3, "verbose-packet", fmt.Sprintf("write=%d bytes=%d", count, n))
	}
}

func (c *sshActivityConn) idleFor() time.Duration {
	return time.Duration(time.Since(sshActivityEpoch).Nanoseconds() - c.receivedAt.Load())
}
