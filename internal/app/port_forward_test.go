package app

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"fmt"
	"io"
	"net"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"golang.org/x/crypto/ssh"
)

func TestParseQuickSSHForwards(t *testing.T) {
	p, err := parseQuickSSH("ssh -p 404 -L 51997:127.0.0.1:51997 root@104.168.105.201")
	if err != nil {
		t.Fatal(err)
	}
	if p.Host != "104.168.105.201" || p.Port != 404 || p.User != "root" {
		t.Fatalf("profile = %+v", p)
	}
	if len(p.Forwards) != 1 || p.Forwards[0] != (PortForward{ListenPort: 51997, RemoteHost: "127.0.0.1", RemotePort: 51997}) {
		t.Fatalf("forwards = %+v", p.Forwards)
	}
	p, err = parseQuickSSH("-L8080:example.com:80 -L 127.0.0.1:9090:10.0.0.5:22 example.com")
	if err != nil {
		t.Fatal(err)
	}
	if len(p.Forwards) != 2 || p.Forwards[0] != (PortForward{ListenPort: 8080, RemoteHost: "example.com", RemotePort: 80}) || p.Forwards[1] != (PortForward{ListenPort: 9090, RemoteHost: "10.0.0.5", RemotePort: 22}) {
		t.Fatalf("forwards = %+v", p.Forwards)
	}
	for _, rejected := range []string{"ssh -R 8080:localhost:80 host.example", "ssh -L 0.0.0.0:8080:example.com:80 host.example", "ssh -L abc host.example", "ssh -L 8080:example.com host.example", "ssh -L 8080:example.com:99999 host.example", "ssh -L 0:host.example:22 host.example", "ssh -L 8080:not a host:22 host.example", "ssh -L"} {
		if _, err := parseQuickSSH(rejected); err == nil {
			t.Fatalf("parseQuickSSH(%q) accepted", rejected)
		}
	}
	command := "host.example"
	for i := 0; i <= maxProfileForwards; i++ {
		command = fmt.Sprintf("-L %d:example.com:22 %s", 10000+i, command)
	}
	if _, err := parseQuickSSH(command); err == nil {
		t.Fatal("forward count above the cap was accepted")
	}
}

func TestProfileForwardsSurviveEditing(t *testing.T) {
	a, err := New(filepath.Join(t.TempDir(), "config"))
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	saved, err := a.store.Save(Profile{Name: "fwd", Host: "example.com", Port: 22, User: "u", Auth: "password", Forwards: []PortForward{{ListenPort: 51997, RemoteHost: "127.0.0.1", RemotePort: 51997}}}, false)
	if err != nil {
		t.Fatal(err)
	}
	if len(saved.Forwards) != 1 {
		t.Fatalf("saved forwards = %+v", saved.Forwards)
	}
	// The connection editor posts no forwards field; the rules survive.
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "renamed", Host: "example.com", Port: 22, User: "u", Auth: "password"}, false); err != nil {
		t.Fatal(err)
	}
	stored, err := a.store.Get(saved.ID)
	if err != nil {
		t.Fatal(err)
	}
	if len(stored.Forwards) != 1 || stored.Forwards[0].ListenPort != 51997 {
		t.Fatalf("editing dropped forwards: %+v", stored.Forwards)
	}
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "renamed", Host: "example.com", Port: 22, User: "u", Auth: "password", Forwards: []PortForward{}}, false); err != nil {
		t.Fatal(err)
	}
	stored, _ = a.store.Get(saved.ID)
	if len(stored.Forwards) != 0 {
		t.Fatalf("explicit empty list did not clear forwards: %+v", stored.Forwards)
	}
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "bad", Host: "example.com", Port: 22, User: "u", Auth: "password", Forwards: []PortForward{{ListenPort: 7, RemoteHost: "example.com", RemotePort: 7}, {ListenPort: 7, RemoteHost: "example.com", RemotePort: 8}}}, false); err == nil {
		t.Fatal("duplicate local ports were accepted")
	}
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "bad", Host: "example.com", Port: 22, User: "u", Auth: "password", Forwards: []PortForward{{ListenPort: 7, RemoteHost: "exa mple.com", RemotePort: 7}}}, false); err == nil {
		t.Fatal("invalid forward target was accepted")
	}
}

// forwardFixture runs an in-process SSH server that only tunnels to one
// registered echo target, plus the plain TCP echo service itself. It returns
// the SSH listen address and the echo port a test should put into
// Profile.Forwards so the direct-tcpip channel is accepted.
func forwardFixture(t *testing.T, password string) (string, int, *App, func()) {
	t.Helper()
	echoListener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	echoHost, echoPortText, _ := net.SplitHostPort(echoListener.Addr().String())
	echoPort, _ := strconv.Atoi(echoPortText)
	var workers sync.WaitGroup
	workers.Add(1)
	go func() {
		defer workers.Done()
		for {
			conn, err := echoListener.Accept()
			if err != nil {
				return
			}
			workers.Add(1)
			go func() {
				defer workers.Done()
				defer conn.Close()
				_, _ = io.Copy(conn, conn)
			}()
		}
	}()
	_, private, _ := ed25519.GenerateKey(rand.Reader)
	signer, _ := ssh.NewSignerFromKey(private)
	config := &ssh.ServerConfig{PasswordCallback: func(_ ssh.ConnMetadata, secret []byte) (*ssh.Permissions, error) {
		if string(secret) == password {
			return nil, nil
		}
		return nil, fmt.Errorf("bad password")
	}}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	workers.Add(1)
	go func() {
		defer workers.Done()
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			workers.Add(1)
			go func() {
				defer workers.Done()
				defer conn.Close()
				server, channels, requests, err := ssh.NewServerConn(conn, config)
				if err != nil {
					return
				}
				defer server.Close()
				go ssh.DiscardRequests(requests)
				for candidate := range channels {
					if candidate.ChannelType() != "direct-tcpip" {
						_ = candidate.Reject(ssh.UnknownChannelType, "this fixture only tunnels")
						continue
					}
					var opened struct {
						Host       string
						Port       uint32
						OriginIP   string
						OriginPort uint32
					}
					// The client must have asked for exactly the echo target;
					// the parsed request is the observable proof of the
					// direct-tcpip payload the session sent.
					if err := ssh.Unmarshal(candidate.ExtraData(), &opened); err != nil || opened.Host != echoHost || opened.Port != uint32(echoPort) {
						_ = candidate.Reject(ssh.Prohibited, "only the echo target is tunneled")
						continue
					}
					channel, channelRequests, err := candidate.Accept()
					if err != nil {
						continue
					}
					workers.Add(1)
					go func() {
						defer workers.Done()
						defer channel.Close()
						go ssh.DiscardRequests(channelRequests)
						target, err := net.Dial("tcp", echoListener.Addr().String())
						if err != nil {
							return
						}
						defer target.Close()
						var copies sync.WaitGroup
						copies.Add(1)
						go func() {
							defer copies.Done()
							_, _ = io.Copy(channel, target)
						}()
						_, _ = io.Copy(target, channel)
						// Half-close the echo socket so its read loop ends;
						// otherwise the reverse copy blocks forever and
						// cleanup's workers.Wait() deadlocks.
						if tcp, ok := target.(*net.TCPConn); ok {
							_ = tcp.CloseWrite()
						}
						copies.Wait()
					}()
				}
			}()
		}
	}()
	a, err := New(filepath.Join(t.TempDir(), "config"))
	if err != nil {
		t.Fatal(err)
	}
	trustFixtureHostKey(t, a, listener.Addr().String(), signer.PublicKey())
	cleanup := func() { a.Close(); listener.Close(); echoListener.Close(); workers.Wait() }
	return listener.Addr().String(), echoPort, a, cleanup
}

func freeLocalPort(t *testing.T) int {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	_, portText, _ := net.SplitHostPort(listener.Addr().String())
	port, _ := strconv.Atoi(portText)
	return port
}

func TestSessionPortForwardEndToEnd(t *testing.T) {
	address, echoPort, a, cleanup := forwardFixture(t, "test-password")
	defer cleanup()
	host, portText, _ := net.SplitHostPort(address)
	port, _ := strconv.Atoi(portText)
	listenPort := freeLocalPort(t)
	p, err := a.store.Save(Profile{Name: "fwd", Host: host, Port: port, User: "tester", Auth: "password", Forwards: []PortForward{{ListenPort: listenPort, RemoteHost: "127.0.0.1", RemotePort: echoPort}}}, false)
	if err != nil {
		t.Fatal(err)
	}
	s, err := a.Connect(context.Background(), p.ID, "test-password")
	if err != nil {
		t.Fatal(err)
	}
	defer a.disconnect(s.ID)
	if len(s.Forwards) != 1 || s.Forwards[0].Error != "" || s.Forwards[0].Listen != fmt.Sprintf("127.0.0.1:%d", listenPort) {
		t.Fatalf("forward status = %+v", s.Forwards)
	}
	for attempt := 0; attempt < 2; attempt++ {
		conn, err := net.Dial("tcp", fmt.Sprintf("127.0.0.1:%d", listenPort))
		if err != nil {
			t.Fatalf("dial forward listener: %v", err)
		}
		message := fmt.Sprintf("hello-%d", attempt)
		if _, err = io.WriteString(conn, message); err != nil {
			t.Fatal(err)
		}
		reply := make([]byte, len(message))
		if _, err = io.ReadFull(conn, reply); err != nil {
			t.Fatal(err)
		}
		conn.Close()
		if string(reply) != message {
			t.Fatalf("echo over the tunnel = %q", reply)
		}
	}
	// Disconnecting must release the listener; the port becomes bindable again.
	a.disconnect(s.ID)
	deadline := time.Now().Add(3 * time.Second)
	for {
		listener, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", listenPort))
		if err == nil {
			listener.Close()
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("forward listener still holds port %d: %v", listenPort, err)
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func TestSessionPortForwardBusyLocalPort(t *testing.T) {
	address, echoPort, a, cleanup := forwardFixture(t, "test-password")
	defer cleanup()
	host, portText, _ := net.SplitHostPort(address)
	port, _ := strconv.Atoi(portText)
	blocker, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer blocker.Close()
	_, busyText, _ := net.SplitHostPort(blocker.Addr().String())
	busyPort, _ := strconv.Atoi(busyText)
	p, err := a.store.Save(Profile{Name: "busy", Host: host, Port: port, User: "tester", Auth: "password", Forwards: []PortForward{{ListenPort: busyPort, RemoteHost: "127.0.0.1", RemotePort: echoPort}}}, false)
	if err != nil {
		t.Fatal(err)
	}
	s, err := a.Connect(context.Background(), p.ID, "test-password")
	if err != nil {
		t.Fatalf("a failed forward bind must not kill the session: %v", err)
	}
	defer a.disconnect(s.ID)
	if len(s.Forwards) != 1 || !strings.Contains(s.Forwards[0].Error, "已被占用") {
		t.Fatalf("forward status = %+v", s.Forwards)
	}
}
