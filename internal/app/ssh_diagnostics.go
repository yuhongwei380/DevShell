package app

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"sync"
	"syscall"
	"time"

	"golang.org/x/crypto/ssh"
)

var sshDiagnosticMessages = map[string]string{
	"DS-100": "用户主动断开、关闭或重新连接",
	"DS-101": "应用退出，关闭连接",
	"DS-110": "连接后未及时打开终端窗口",
	"DS-120": "SSH 终端初始化超时",
	"DS-201": "SSH 连续 90 秒未收到数据，连接超时",
	"DS-202": "SSH 底层传输结束或出现网络错误",
	"DS-210": "SSH 终端通道结束，请结合日志中的传输状态判断原因",
	"DS-211": "无法向远端终端写入数据",
	"DS-212": "无法创建或启动远端终端",
	"DS-220": "本地终端窗口通信通道关闭",
	"DS-221": "向本地终端窗口发送数据失败或超时",
	"DS-230": "后台 SSH 通道取消后，30 秒内仍未完成建立或清理",
	"DS-231": "后台 SSH 命令取消后，30 秒内仍未完成清理",
	"DS-240": "SFTP 操作取消或超时后，30 秒内仍未完成清理",
	"DS-299": "连接关闭，未收到更具体的触发原因",
}

// Diagnostics contain fixed categories and timing only: never command text,
// SSH input/output, paths on the server, addresses, credentials or raw errors.
type SSHDisconnectDiagnostic struct {
	TraceID        string    `json:"traceId"`
	Code           string    `json:"code"`
	Message        string    `json:"message"`
	Detail         string    `json:"detail,omitempty"`
	At             time.Time `json:"at"`
	AgeSeconds     float64   `json:"ageSeconds"`
	LogPath        string    `json:"logPath,omitempty"`
	LogWriteFailed bool      `json:"logWriteFailed,omitempty"`
}

type sshDiagnosticEvent struct {
	SSHDisconnectDiagnostic
	InboundIdleSeconds      float64 `json:"inboundIdleSeconds,omitempty"`
	Kind                    string  `json:"kind"`
	HeartbeatPending        bool    `json:"heartbeatPending,omitempty"`
	HeartbeatPendingSeconds float64 `json:"heartbeatPendingSeconds,omitempty"`
	HeartbeatMilliseconds   float64 `json:"heartbeatMilliseconds,omitempty"`
	HeartbeatAgeSeconds     float64 `json:"heartbeatAgeSeconds,omitempty"`
}

type sshDiagnosticLog struct {
	mu            sync.Mutex
	path          string
	closed        bool
	records       map[string]SSHDisconnectDiagnostic
	order         []string
	pending       map[string][]sshDiagnosticEvent
	persisted     map[string]bool
	verboseEvents map[string][]sshVerboseEvent
	verboseNext   map[string]uint64
}

// sshVerboseEvent contains only existing, bounded diagnostic categories and
// timing. It never carries terminal data or arbitrary server-provided text.
type sshVerboseEvent struct {
	Sequence   uint64  `json:"sequence"`
	Kind       string  `json:"kind"`
	Detail     string  `json:"detail,omitempty"`
	AgeSeconds float64 `json:"ageSeconds"`
}

// normalSSHDiagnosticCodes mark closes that need no on-disk trace: user exit,
// tab close, reconnect, or application quit. Their events stay in memory only;
// the log file is created solely once an abnormal code appears.
var normalSSHDiagnosticCodes = map[string]bool{"DS-100": true, "DS-101": true}

// sshDiagnosticPendingLimit bounds the in-memory buffer kept per session while
// it is still undecided whether anything will be written at all.
const sshDiagnosticPendingLimit = 512

// close waits for the current append and prevents late transport callbacks
// from recreating log files after App.Close has returned.
func (l *sshDiagnosticLog) close() { l.mu.Lock(); l.closed = true; l.mu.Unlock() }

func (l *sshDiagnosticLog) record(id string, event sshDiagnosticEvent) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.closed {
		return
	}
	if l.records == nil {
		l.records = make(map[string]SSHDisconnectDiagnostic)
		l.pending = make(map[string][]sshDiagnosticEvent)
		l.persisted = make(map[string]bool)
	}
	if _, found := l.records[id]; !found {
		l.order = append(l.order, id)
		if len(l.order) > 256 {
			evicted := l.order[0]
			l.order = l.order[1:]
			delete(l.records, evicted)
			delete(l.pending, evicted)
			delete(l.persisted, evicted)
			delete(l.verboseEvents, evicted)
			delete(l.verboseNext, evicted)
		}
	}
	previous := l.records[id]
	// A network error produced by our own close cannot replace its trigger.
	if previous.Code == "" || event.Kind == "disconnect" && previous.Code == "DS-299" {
		previous = event.SSHDisconnectDiagnostic
	}
	if !l.persisted[id] {
		if event.Kind == "disconnect" && event.Code != "" && !normalSSHDiagnosticCodes[event.Code] {
			// The first abnormal code promotes the session: everything buffered
			// for it, plus this event, goes to disk now.
			l.persisted[id] = true
			for _, buffered := range l.pending[id] {
				if l.path != "" {
					if err := l.append(buffered); err != nil {
						previous.LogWriteFailed = true
					}
				}
			}
			delete(l.pending, id)
		} else {
			// Normal closes and pre-disconnect events never touch the disk: a
			// session that only ever ends normally leaves no log file behind.
			buffer := append(l.pending[id], event)
			if len(buffer) > sshDiagnosticPendingLimit {
				buffer = append([]sshDiagnosticEvent(nil), buffer[len(buffer)-sshDiagnosticPendingLimit:]...)
			}
			l.pending[id] = buffer
			l.records[id] = previous
			return
		}
	}
	if l.path != "" {
		if err := l.append(event); err != nil {
			previous.LogWriteFailed = true
		}
		previous.LogPath = l.path
	}
	l.records[id] = previous
}

// recordImmediate writes an event to disk right away, bypassing the buffer
// that defers writes until a session shows an abnormal close. -vvv verbose
// tracing is explicitly requested by the user, so it must survive even a
// normal close that would otherwise leave no log file behind.
func (l *sshDiagnosticLog) recordImmediate(id string, event sshDiagnosticEvent) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.closed {
		return
	}
	if l.records == nil {
		l.records = make(map[string]SSHDisconnectDiagnostic)
		l.pending = make(map[string][]sshDiagnosticEvent)
		l.persisted = make(map[string]bool)
	}
	if _, found := l.records[id]; !found {
		l.order = append(l.order, id)
		if len(l.order) > 256 {
			evicted := l.order[0]
			l.order = l.order[1:]
			delete(l.records, evicted)
			delete(l.pending, evicted)
			delete(l.persisted, evicted)
			delete(l.verboseEvents, evicted)
			delete(l.verboseNext, evicted)
		}
	}
	previous := l.records[id]
	if l.verboseEvents == nil {
		l.verboseEvents = make(map[string][]sshVerboseEvent)
		l.verboseNext = make(map[string]uint64)
	}
	l.verboseNext[id]++
	events := append(l.verboseEvents[id], sshVerboseEvent{Sequence: l.verboseNext[id], Kind: event.Kind, Detail: event.Detail, AgeSeconds: event.AgeSeconds})
	if len(events) > 512 {
		events = append([]sshVerboseEvent(nil), events[len(events)-512:]...)
	}
	l.verboseEvents[id] = events
	if !l.persisted[id] {
		// Promotion flushes anything this session had already buffered.
		l.persisted[id] = true
		for _, buffered := range l.pending[id] {
			if l.path != "" {
				if err := l.append(buffered); err != nil {
					previous.LogWriteFailed = true
				}
			}
			delete(l.pending, id)
		}
	}
	if l.path != "" {
		if err := l.append(event); err != nil {
			previous.LogWriteFailed = true
		}
		previous.LogPath = l.path
	}
	l.records[id] = previous
}

func (l *sshDiagnosticLog) append(event sshDiagnosticEvent) error {
	if err := os.MkdirAll(filepath.Dir(l.path), 0700); err != nil {
		return err
	}
	// Keep the active file and two older files, about 3 MiB in total.
	if info, err := os.Lstat(l.path); err == nil {
		if !info.Mode().IsRegular() {
			return errors.New("diagnostic log is not a regular file")
		}
		if info.Size() >= 1<<20 {
			_ = os.Remove(l.path + ".2")
			if _, err := os.Lstat(l.path + ".1"); err == nil {
				if err = os.Rename(l.path+".1", l.path+".2"); err != nil {
					return err
				}
			}
			if err = os.Rename(l.path, l.path+".1"); err != nil {
				return err
			}
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	f, err := os.OpenFile(l.path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer f.Close()
	return json.NewEncoder(f).Encode(event)
}

func (s *Session) diagnosticEvent(kind, code, detail string) sshDiagnosticEvent {
	now := time.Now()
	trace := s.ID
	if len(trace) > 12 {
		trace = trace[:12]
	}
	age := float64(0)
	if !s.diagnosticStarted.IsZero() {
		age = now.Sub(s.diagnosticStarted).Seconds()
	}
	e := sshDiagnosticEvent{SSHDisconnectDiagnostic: SSHDisconnectDiagnostic{TraceID: trace, Code: code, Message: sshDiagnosticMessages[code], Detail: detail, At: now, AgeSeconds: age}, Kind: kind}
	if s.activity != nil {
		e.InboundIdleSeconds = s.activity.idleFor().Seconds()
	}
	s.latencyMu.RLock()
	sample := s.latency
	s.latencyMu.RUnlock()
	e.HeartbeatPending = sample.Pending
	if sample.Pending && !sample.PendingSince.IsZero() {
		e.HeartbeatPendingSeconds = now.Sub(sample.PendingSince).Seconds()
	}
	if !sample.SampledAt.IsZero() {
		e.HeartbeatAgeSeconds = now.Sub(sample.SampledAt).Seconds()
		e.HeartbeatMilliseconds = sample.Milliseconds
	}
	return e
}

func (s *Session) noteDisconnect(code, detail string) {
	if s.diagnosticLog == nil {
		return
	}
	s.diagnosticMu.Lock()
	defer s.diagnosticMu.Unlock()
	if s.diagnosticCode != "" && (s.diagnosticCode != "DS-299" || code == "DS-299") {
		return
	}
	s.diagnosticCode = code
	if s.verboseLevel > 0 {
		s.diagnosticLog.recordImmediate(s.ID, s.diagnosticEvent("verbose-disconnect", code, code+":"+detail))
	}
	s.diagnosticLog.record(s.ID, s.diagnosticEvent("disconnect", code, detail))
}
func (s *Session) closeDiagnostic(code, detail string) { s.noteDisconnect(code, detail); s.Close() }
func (s *Session) forceCloseDiagnostic(code, detail string) {
	s.noteDisconnect(code, detail)
	s.forceClose()
}

func (s *Session) noteOperationCancellation(stage string, err error) {
	if s.diagnosticLog != nil {
		s.diagnosticLog.record(s.ID, s.diagnosticEvent("operation-cancelled", "", stage+":"+sshDiagnosticErrorKind(err)))
	}
}

// noteTerminalStage records one failed terminal startup request. Only the stage
// and the fixed error category are written: never the request text or its error.
func (s *Session) noteTerminalStage(stage string, err error) {
	s.noteTerminalStageKind(stage, sshDiagnosticErrorKind(err))
}

// noteTerminalStageKind records a terminal startup stage decision that had no
// error of its own, such as refusing to send an oversized request.
func (s *Session) noteTerminalStageKind(stage, kind string) {
	if s.diagnosticLog != nil {
		s.diagnosticLog.record(s.ID, s.diagnosticEvent("terminal-stage", "", stage+":"+kind))
	}
}

// noteVerbose records one -vvv diagnostics event at the given level. These
// events go straight to disk: the user asked for them.
func (s *Session) noteVerbose(level int, kind, detail string) {
	if s.diagnosticLog == nil || s.verboseLevel < level {
		return
	}
	s.diagnosticLog.recordImmediate(s.ID, s.diagnosticEvent(kind, "", detail))
}

// noteVerboseHandshakeFailure keeps the authentication-method trace from a
// failed -vvv attempt. The library tears the transport down on failure, so the
// server version string is unreachable; the offered/tried/partial methods are
// exactly what -vvv debugging on embedded servers is after.
func (a *App) noteVerboseHandshakeFailure(profileID, detail string) {
	trace := profileID
	if len(trace) > 12 {
		trace = trace[:12]
	}
	a.sshDiagnostics.recordImmediate("handshake-"+trace, sshDiagnosticEvent{SSHDisconnectDiagnostic: SSHDisconnectDiagnostic{TraceID: trace, Detail: detail, At: time.Now()}, Kind: "verbose-handshake-failed"})
}

func (s *Session) startConnectionDiagnostics() {
	if s.diagnosticLog == nil {
		return
	}
	s.diagnosticLog.record(s.ID, s.diagnosticEvent("connected", "", ""))
	go func() {
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-s.ctx.Done():
				return
			case <-ticker.C:
				event := s.diagnosticEvent("sample", "", "")
				if s.verboseLevel > 0 {
					s.diagnosticLog.recordImmediate(s.ID, event)
				} else {
					s.diagnosticLog.record(s.ID, event)
				}
			}
		}
	}()
	go func() {
		err := s.client.Wait()
		kind := sshDiagnosticErrorKind(err)
		s.noteVerbose(1, "verbose-transport-end", kind)
		s.noteDisconnect("DS-202", kind)
		s.diagnosticLog.record(s.ID, s.diagnosticEvent("transport-end", "", kind))
		s.Close()
	}()
}

// sshShellExitDiagnostic classifies a finished remote shell: an exit status
// (including non-zero) means the shell ended on its own, which is a normal
// user-initiated close; anything else is a channel or transport anomaly.
func sshShellExitDiagnostic(err error) (string, string) {
	var exitStatus *ssh.ExitError
	if err == nil || errors.As(err, &exitStatus) {
		return "DS-100", "remote-shell-exit"
	}
	return "DS-210", sshDiagnosticErrorKind(err)
}

func sshDiagnosticErrorKind(err error) string {
	if err == nil {
		return "completed"
	}
	if errors.Is(err, io.EOF) {
		return "eof"
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return "deadline-exceeded"
	}
	if errors.Is(err, context.Canceled) {
		return "context-cancelled"
	}
	if errors.Is(err, syscall.ECONNRESET) {
		return "connection-reset"
	}
	if errors.Is(err, syscall.ECONNABORTED) {
		return "connection-aborted"
	}
	if errors.Is(err, syscall.EPIPE) {
		return "broken-pipe"
	}
	if errors.Is(err, net.ErrClosed) {
		return "local-socket-closed"
	}
	// The SSH package keeps disconnectMsg private. Identify the actual type,
	// then match only fixed server messages; never persist arbitrary peer text.
	for current := err; current != nil; current = errors.Unwrap(current) {
		typ := reflect.TypeOf(current)
		if typ.Kind() == reflect.Pointer && typ.Elem().PkgPath() == "golang.org/x/crypto/ssh" && typ.Elem().Name() == "disconnectMsg" {
			message := current.Error()
			for _, reason := range []int{2, 3, 11} {
				for _, known := range []struct{ message, kind string }{{"Idle timeout", "server-idle-timeout"}, {"Keepalive timeout", "server-keepalive-timeout"}, {"String too long", "server-request-too-long"}} {
					if message == "ssh: disconnect, reason "+strconv.Itoa(reason)+": "+strconv.Quote(known.message) {
						return known.kind
					}
				}
			}
			return "server-disconnect"
		}
	}
	var network net.Error
	if errors.As(err, &network) && network.Timeout() {
		return "network-timeout"
	}
	var exit *ssh.ExitError
	if errors.As(err, &exit) {
		return "remote-exit-status"
	}
	var missing *ssh.ExitMissingError
	if errors.As(err, &missing) {
		return "remote-channel-ended-without-status"
	}
	return "transport-or-channel-error"
}

func (a *App) sshDisconnectDiagnosticHTTP(w http.ResponseWriter, r *http.Request) {
	a.sshDiagnostics.mu.Lock()
	value, ok := a.sshDiagnostics.records[r.PathValue("id")]
	a.sshDiagnostics.mu.Unlock()
	if !ok {
		writeJSON(w, map[string]string{"code": "", "message": "尚无断开记录"})
		return
	}
	writeJSON(w, value)
}

// Verbose traces are session-scoped, bounded, and available after disconnect.
func (a *App) sshVerboseHTTP(w http.ResponseWriter, r *http.Request) {
	after, _ := strconv.ParseUint(r.URL.Query().Get("after"), 10, 64)
	id := r.PathValue("id")
	a.sshDiagnostics.mu.Lock()
	events := make([]sshVerboseEvent, 0)
	for _, e := range a.sshDiagnostics.verboseEvents[id] {
		if e.Sequence > after {
			events = append(events, e)
		}
	}
	a.sshDiagnostics.mu.Unlock()
	writeJSON(w, map[string]any{"events": events})
}
