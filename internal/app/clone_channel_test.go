package app

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/json"
	"io"
	"net"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync/atomic"
	"testing"
	"testing/fstest"
	"time"

	"github.com/gorilla/websocket"
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
	if clone.Home != "/" {
		t.Fatalf("clone home = %q, want \"/\"", clone.Home)
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

// 端到端回归(实际缺陷):克隆标签通过 WebSocket 接入终端时,relay 必须在共享
// 传输上打开自己的 SSH 会话渠道并正常就绪、收发回显;同一会话的第二次无交接
// 接入被 409「此会话已有终端」拒绝(前端 createTerminal 已自动接入,手动再接
// 就会把新标签打成红 DS-290)。克隆终端关闭后,只剩它自己的渠道消亡,父会话
// 与传输原样保留。
func TestCloneSessionChannelTerminalAttachesOnSharedTransport(t *testing.T) {
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
	sessionChannels := &atomic.Int32{}
	ptyChannels := &atomic.Int32{}
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func() {
				server, channels, requests, err := ssh.NewServerConn(conn, config)
				if err != nil {
					conn.Close()
					return
				}
				defer server.Close()
				go func() {
					for request := range requests {
						request.Reply(false, nil)
					}
				}()
				for newChannel := range channels {
					if newChannel.ChannelType() != "session" {
						newChannel.Reject(ssh.UnknownChannelType, "only session channels")
						continue
					}
					session, reqs, err := newChannel.Accept()
					if err != nil {
						continue
					}
					sessionChannels.Add(1)
					go func() {
						for request := range reqs {
							switch request.Type {
							case "pty-req":
								ptyChannels.Add(1)
								if request.WantReply {
									request.Reply(true, nil)
								}
							case "exec", "shell", "window-change":
								if request.WantReply {
									request.Reply(true, nil)
								}
							default:
								if request.WantReply {
									request.Reply(false, nil)
								}
							}
						}
					}()
					go func() { _, _ = io.Copy(session, session) }() // 回显终端字节
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
	httpServer := httptest.NewServer(a.Handler(fstest.MapFS{}))
	t.Cleanup(func() {
		owner.forceClose()
		httpServer.Close()
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

	address := "ws" + strings.TrimPrefix(httpServer.URL, "http") + "/api/sessions/" + clone.ID + "/terminal?token=" + url.QueryEscape(a.Token()) + "&cols=100&rows=30"
	ws, _, err := websocket.DefaultDialer.Dial(address, nil)
	if err != nil {
		t.Fatalf("clone terminal attach: %v", err)
	}
	defer ws.Close()
	relayUntil := func(predicate func(kind int, data []byte) bool) (int, []byte) {
		t.Helper()
		for {
			ws.SetReadDeadline(time.Now().Add(8 * time.Second))
			kind, data, err := ws.ReadMessage()
			if err != nil {
				t.Fatalf("clone terminal read: %v", err)
			}
			if kind == websocket.BinaryMessage {
				if err := ws.WriteJSON(relayMessage{Type: "ack"}); err != nil {
					t.Fatalf("clone terminal ack: %v", err)
				}
			}
			if predicate(kind, data) {
				return kind, data
			}
		}
	}
	relayUntil(func(kind int, data []byte) bool {
		return kind == websocket.TextMessage && json.Valid(data) && strings.Contains(string(data), `"type":"ready"`)
	})
	if err := ws.WriteJSON(relayMessage{Type: "input", Data: "clone-echo-me\r"}); err != nil {
		t.Fatalf("clone terminal input: %v", err)
	}
	relayUntil(func(kind int, data []byte) bool {
		return kind == websocket.BinaryMessage && bytes.Contains(data, []byte("clone-echo-me"))
	})

	// prepareTerminalLaunch 的 staging(sftp 上传、mkdir/ln)会在终端 exec 前打开
	// 若干非 PTY 渠道;先等 fixture 计数静止,再验证被拒的二次接入不再增加渠道。
	settled := sessionChannels.Load()
	stableSince := time.Now()
	settleDeadline := time.Now().Add(5 * time.Second)
	for time.Since(stableSince) < 200*time.Millisecond {
		if time.Now().After(settleDeadline) {
			t.Fatal("fixture channel count never settles")
		}
		time.Sleep(20 * time.Millisecond)
		if now := sessionChannels.Load(); now != settled {
			settled = now
			stableSince = time.Now()
		}
	}
	// 同一克隆会话的第二次接入必须被拒(这正是 rc.3 前端二次 openSocket 撞坏的
	// 路径):409 而非第二条终端渠道。
	second, res, err := websocket.DefaultDialer.Dial(address, nil)
	if second != nil {
		second.Close()
	}
	if err == nil || res == nil || res.StatusCode != 409 {
		t.Fatalf("second attach to the clone was not rejected with 409: %v %v", err, res)
	}
	if got := ptyChannels.Load(); got != 1 {
		t.Fatalf("clone terminal opened %d PTY session channels, want 1", got)
	}
	time.Sleep(150 * time.Millisecond)
	if got := sessionChannels.Load(); got != settled {
		t.Fatalf("rejected second attach still opened SSH channels: before=%d after=%d", settled, got)
	}

	// 关闭克隆终端标签:仅克隆自身断开,父会话与共享传输原样保留。
	ws.Close()
	deadline := time.Now().Add(3 * time.Second)
	for {
		if _, err := a.session(clone.ID); err != nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("closing the clone terminal kept the clone session alive")
		}
		time.Sleep(20 * time.Millisecond)
	}
	if _, err := a.session(owner.ID); err != nil {
		t.Fatalf("owner lost when the clone terminal closed: %v", err)
	}
	transport.mu.Lock()
	refs, transportClosed := transport.refs, transport.closed
	transport.mu.Unlock()
	if refs != 1 || transportClosed {
		t.Fatalf("clone terminal closed the shared transport: refs=%d closed=%v", refs, transportClosed)
	}
	if _, ok, err := client.SendRequest("keepalive@devshell.test", true, nil); err != nil {
		t.Fatalf("transport died with the clone terminal: ok=%v err=%v", ok, err)
	}
}
