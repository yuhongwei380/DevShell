package app

// Local port forwarding (ssh -L). A profile's Forwards open loopback-only
// listeners after a successful SSH authentication; every accepted local
// connection is relayed through the session's own *ssh.Client via a
// direct-tcpip channel. Listeners live and die with the session: forceClose
// closes them, so a disconnect never leaks a local port. Bind addresses stay
// on the loopback interface — exposing a forward to the LAN is intentionally
// not offered. -R and -D are not implemented.

import (
	"errors"
	"fmt"
	"io"
	"net"
	"strconv"
	"strings"
	"sync"
	"syscall"

	"golang.org/x/crypto/ssh"
)

const maxProfileForwards = 16

// PortForward is one ssh -L [bind:]port:host:port rule saved on a Profile.
// The bind address is validated to be loopback at parse time and never stored;
// listeners always bind 127.0.0.1.
type PortForward struct {
	ListenPort int    `json:"listenPort"`
	RemoteHost string `json:"remoteHost"`
	RemotePort int    `json:"remotePort"`
}

func (f PortForward) listenAddress() string {
	return net.JoinHostPort("127.0.0.1", strconv.Itoa(f.ListenPort))
}
func (f PortForward) remoteAddress() string {
	return net.JoinHostPort(f.RemoteHost, strconv.Itoa(f.RemotePort))
}

const portForwardUsage = "端口转发格式应为 本地端口:目标主机:目标端口，例如 -L 51997:127.0.0.1:51997"

// parsePortForward parses one -L argument: [bind:]local_port:host:port. A bind
// prefix is accepted only when it means this machine (OpenSSH users commonly
// write it); anything else is rejected rather than silently widened.
func parsePortForward(spec string) (PortForward, error) {
	parts := strings.Split(spec, ":")
	bind := ""
	switch len(parts) {
	case 3:
	case 4:
		bind, parts = parts[0], parts[1:]
	default:
		return PortForward{}, errors.New(portForwardUsage)
	}
	listenPort, err := strconv.Atoi(parts[0])
	if err != nil || listenPort < 1 || listenPort > 65535 {
		return PortForward{}, errors.New("端口转发的本地端口应为 1–65535")
	}
	remotePort, err := strconv.Atoi(parts[2])
	if err != nil || remotePort < 1 || remotePort > 65535 {
		return PortForward{}, errors.New("端口转发的目标端口应为 1–65535")
	}
	if !validQuickHost(parts[1]) {
		return PortForward{}, errors.New("端口转发的目标主机应为主机名或 IP")
	}
	if bind != "" && bind != "127.0.0.1" && bind != "localhost" && bind != "::1" {
		return PortForward{}, errors.New("端口转发只监听本机，监听地址可省略；不支持向局域网开放")
	}
	return PortForward{ListenPort: listenPort, RemoteHost: parts[1], RemotePort: remotePort}, nil
}

// validateProfileForwards guards forwards that arrive from stored or edited
// profiles, where no parse-time normalization applies.
func validateProfileForwards(list []PortForward) error {
	if len(list) > maxProfileForwards {
		return fmt.Errorf("端口转发最多 %d 条", maxProfileForwards)
	}
	seen := map[int]bool{}
	for _, f := range list {
		if f.ListenPort < 1 || f.ListenPort > 65535 || f.RemotePort < 1 || f.RemotePort > 65535 || !validQuickHost(f.RemoteHost) {
			return errors.New("端口转发配置无效，请检查端口与目标主机")
		}
		if seen[f.ListenPort] {
			return fmt.Errorf("端口转发的本地端口 %d 重复", f.ListenPort)
		}
		seen[f.ListenPort] = true
	}
	return nil
}

// forwardStatus is reported on the session JSON so the terminal can echo the
// outcome right after connecting, the way ssh -L reports a failed bind.
type forwardStatus struct {
	Listen string `json:"listen"`
	Remote string `json:"remote"`
	Error  string `json:"error,omitempty"`
}

type forwardListener struct {
	listener net.Listener
	spec     PortForward
}

func forwardListenError(port int, err error) string {
	// Windows reports bind failures with Winsock codes (WSAEADDRINUSE 10048,
	// WSAEACCES 10013); syscall.EADDRINUSE/EACCES carry different POSIX
	// numbers there, so errors.Is alone misses them. Match both worlds.
	if errors.Is(err, syscall.EADDRINUSE) || errorsIsErrno(err, 10048) {
		return fmt.Sprintf("本机端口 %d 已被占用", port)
	}
	if errors.Is(err, syscall.EACCES) || errorsIsErrno(err, 10013) {
		return fmt.Sprintf("本机端口 %d 需要管理员权限", port)
	}
	return err.Error()
}

// errorsIsErrno matches a raw numeric errno anywhere in the error chain —
// used only for the Winsock values that do not exist as constants on Unix.
func errorsIsErrno(err error, number int) bool {
	var errno syscall.Errno
	return errors.As(err, &errno) && int(errno) == number
}

// startForwards binds every listener synchronously so the connect response
// already carries the outcome; a bound listener then serves in its own
// goroutine until stopForwards closes it. Failure of one forward never
// cancels the session — the terminal remains usable with a clear warning.
func (s *Session) startForwards(specs []PortForward) {
	for _, spec := range specs {
		status := forwardStatus{Listen: spec.listenAddress(), Remote: spec.remoteAddress()}
		listener, err := net.Listen("tcp", status.Listen)
		if err == nil {
			s.forwardsMu.Lock()
			// forceClose may already have stopped forwards while we were
			// binding; never hand a fresh listener to a dead session.
			if s.forwardersStopped {
				s.forwardsMu.Unlock()
				_ = listener.Close()
				status.Error = "会话已关闭"
			} else {
				s.forwarders = append(s.forwarders, &forwardListener{listener: listener, spec: spec})
				s.forwardsMu.Unlock()
				go s.serveForward(listener, spec)
			}
		} else {
			status.Error = forwardListenError(spec.ListenPort, err)
		}
		s.forwardsMu.Lock()
		s.Forwards = append(s.Forwards, status)
		s.forwardsMu.Unlock()
		if s.verboseLevel > 0 {
			s.noteVerbose(2, "verbose-forward", fmt.Sprintf("listen=%s remote=%s error=%s", status.Listen, status.Remote, status.Error))
		}
	}
}

func (s *Session) serveForward(listener net.Listener, spec PortForward) {
	for {
		conn, err := listener.Accept()
		if err != nil {
			return // stopForwards closed the listener
		}
		go relayForward(s.client, spec, conn)
	}
}

func relayForward(client *ssh.Client, spec PortForward, conn net.Conn) {
	defer conn.Close()
	// A refused tunnel (for example the server sets AllowTcpForwarding no)
	// surfaces here as an error; the local connection simply ends, the same
	// observable behavior as ssh -L rejecting the channel on connect.
	// Client.Dial's first argument is the network, not an address: it opens
	// the direct-tcpip channel itself (x/crypto ssh tcpip.go).
	remote, err := client.Dial("tcp", spec.remoteAddress())
	if err != nil {
		return
	}
	defer remote.Close()
	var copies sync.WaitGroup
	copies.Add(1)
	go func() {
		defer copies.Done()
		_, _ = io.Copy(remote, conn)
		if writable, ok := remote.(ssh.Channel); ok {
			_ = writable.CloseWrite()
		}
	}()
	_, _ = io.Copy(conn, remote)
	if half, ok := conn.(interface{ CloseWrite() error }); ok {
		_ = half.CloseWrite()
	}
	copies.Wait()
}

func (s *Session) stopForwards() {
	s.forwardsMu.Lock()
	defer s.forwardsMu.Unlock()
	s.forwardersStopped = true
	for _, f := range s.forwarders {
		_ = f.listener.Close()
	}
	s.forwarders = nil
}
