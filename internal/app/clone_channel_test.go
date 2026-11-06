package app

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"net"
	"sync/atomic"
	"testing"
	"time"

	"golang.org/x/crypto/ssh"
)

// Xshell 语义回归：复制同服务器SSH会话复用同一条 SSH 连接，只有最后一个标签
// 关闭时才断开传输。测试服务器统计握手次数：克隆必须不再拨号或认证。
func TestCloneSessionChannelSharesOneSSHConnection(t *testing.T) {
	t.Parallel()
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	_, key, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	signer, err := ssh.NewSignerFromKey(key)
	if err != nil {
		t.Fatal(err)
	}
	config := &ssh.ServerConfig{NoClientAuth: true}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	dials := &atomic.Int32{}
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func() {
				dials.Add(1)
				server, channels, requests, err := ssh.NewServerConn(conn, config)
				if err != nil {
					conn.Close()
					return
				}
				defer server.Close()
				go func() {
					for ch := range channels {
						ch.Reject(ssh.UnknownChannelType, "unused")
					}
				}()
				for request := range requests {
					request.Reply(false, nil)
				}
			}()
		}
	}()
	raw, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	activity := newSSHActivityConn(raw)
	conn, channels, requests, err := ssh.NewClientConn(activity, listener.Addr().String(), &ssh.ClientConfig{User: "fixture", HostKeyCallback: ssh.FixedHostKey(signer.PublicKey())})
	if err != nil {
		raw.Close()
		t.Fatal(err)
	}
	client := ssh.NewClient(conn, channels, requests)
	ctx, cancel := context.WithCancel(context.Background())
	transport := &sshTransport{client: client, activity: activity, refs: 1}
	owner := &Session{ID: randomID(), ctx: ctx, cancel: cancel, client: client, activity: activity, transport: transport, connectionHost: listener.Addr().String(), ProfileID: "owner-profile"}
	owner.diagnosticLog = &a.sshDiagnostics
	a.mu.Lock()
	a.sessions[owner.ID] = owner
	a.mu.Unlock()
	t.Cleanup(func() {
		owner.forceClose()
		listener.Close()
		select {
		case <-done:
		case <-time.After(2 * time.Second):
			t.Error("ssh accept worker leaked")
		}
	})

	clone, err := a.CloneSessionChannel(context.Background(), owner.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer clone.forceClose()
	if clone.ID == owner.ID || clone.transport != transport || clone.ProfileID != owner.ProfileID {
		t.Fatalf("clone identity: id=%q owner=%q shared-transport=%v", clone.ID, owner.ID, clone.transport == transport)
	}
	transport.mu.Lock()
	refs := transport.refs
	transport.mu.Unlock()
	if refs != 2 {
		t.Fatalf("transport refs after clone = %d, want 2", refs)
	}
	if got := dials.Load(); got != 1 {
		t.Fatalf("clone re-dialled: server handshakes = %d, want 1", got)
	}
	if _, err := a.session(clone.ID); err != nil {
		t.Fatalf("clone not published: %v", err)
	}

	// 关闭克隆：传输必须留给父会话；引用计数回到 1，父会话仍可发全局请求。
	clone.forceClose()
	transport.mu.Lock()
	refs = transport.refs
	transportClosed := transport.closed
	transport.mu.Unlock()
	if refs != 1 || transportClosed {
		t.Fatalf("closing the clone released the transport: refs=%d closed=%v", refs, transportClosed)
	}
	if _, err := a.session(owner.ID); err != nil {
		t.Fatalf("owner lost after clone close: %v", err)
	}
	if _, err := a.session(clone.ID); err == nil {
		t.Fatal("clone session survived its own close")
	}
	if _, ok, err := client.SendRequest("keepalive@devshell.test", true, nil); err != nil {
		t.Fatalf("transport died with the clone: ok=%v err=%v", ok, err)
	}

	// 关闭父会话：最后一个引用释放传输，克隆经 Wait() 感知断连并自行退出。
	owner.forceClose()
	if _, _, err := client.SendRequest("keepalive@devshell.test", true, nil); err == nil {
		t.Fatal("transport still alive after the last session closed")
	}
	deadline := time.Now().Add(2 * time.Second)
	for {
		if _, err := a.session(clone.ID); err != nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("clone outlived the shared transport")
		}
		time.Sleep(20 * time.Millisecond)
	}
	if _, err := a.CloneSessionChannel(context.Background(), owner.ID); err == nil {
		t.Fatal("clone succeeded for a closed parent session")
	}
}
