package app

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"golang.org/x/text/encoding/simplifiedchinese"
)

// Local-prompt network probes run while no session is connected. They never
// execute a user shell command: nslookup, dig and curl are implemented with
// the Go standard library, and traceroute only ever invokes the fixed system
// tool with a strictly validated host (windowless, GBK-decoded on Windows).
// Ctrl+C in the frontend aborts the request; cancelling r.Context() then
// terminates the child process or in-flight lookup.
var localProbeSlots = make(chan struct{}, 2)

const localProbeOutputLimit = 256 << 10

type limitedBuffer struct {
	buf   bytes.Buffer
	limit int
}

func (b *limitedBuffer) Write(p []byte) (int, error) {
	if room := b.limit - b.buf.Len(); room > 0 {
		if len(p) > room {
			b.buf.Write(p[:room])
		} else {
			b.buf.Write(p)
		}
	}
	return len(p), nil
}

func acquireLocalProbe() (func(), error) {
	select {
	case localProbeSlots <- struct{}{}:
		return func() { <-localProbeSlots }, nil
	default:
		return nil, errors.New("本机探测繁忙，请稍后重试")
	}
}

func localProbeHost(w http.ResponseWriter, r *http.Request, verb string) (string, bool) {
	var input struct {
		Host string `json:"host"`
	}
	if !decode(w, r, &input) {
		return "", false
	}
	host := strings.Trim(strings.TrimSpace(input.Host), "[]")
	if !validQuickHost(host) {
		respond(w, nil, fmt.Errorf("%s 后只接受主机名或 IP", verb))
		return "", false
	}
	return host, true
}

// decodeProbeOutput keeps UTF-8 output as-is and converts GBK (the default
// console code page on zh-CN Windows) so tracert's Chinese status text such
// as 「请求超时」 no longer renders as replacement diamonds.
func decodeProbeOutput(raw []byte) string {
	if utf8.Valid(raw) {
		return string(raw)
	}
	if decoded, err := simplifiedchinese.GBK.NewDecoder().Bytes(raw); err == nil {
		return string(decoded)
	}
	return strings.ToValidUTF8(string(raw), "\uFFFD")
}

var digRecordTypes = map[string]bool{"A": true, "AAAA": true, "CNAME": true, "MX": true, "TXT": true, "NS": true}

// probeStream emits {"c": "..."} NDJSON frames as output arrives, then one
// {"done": true, "ok": ...} frame. Writes are best effort: once the client
// aborted, encoding fails and the stream is marked closed; the request
// context terminates the child process separately.
type probeStream struct {
	mu        sync.Mutex
	enc       *json.Encoder
	flusher   http.Flusher
	sent      int
	truncated bool
	closed    bool
}

func (s *probeStream) chunk(text string) {
	if text == "" {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	if room := localProbeOutputLimit - s.sent; room <= 0 {
		if !s.truncated {
			s.truncated = true
			_ = s.enc.Encode(map[string]string{"c": "\n…（输出超过 256 KiB，已截断）"})
		}
		return
	} else if len(text) > room {
		text = text[:room]
	}
	s.sent += len(text)
	if err := s.enc.Encode(map[string]string{"c": text}); err != nil {
		s.closed = true
		return
	}
	s.flusher.Flush()
}

func (s *probeStream) done(ok bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	if err := s.enc.Encode(map[string]any{"done": true, "ok": ok}); err != nil {
		s.closed = true
		return
	}
	s.flusher.Flush()
	s.closed = true
}

// probeLineWriter forwards whole lines to the stream so GBK decoding never
// splits a multibyte character and the UI never renders half a hop line.
// stdout and stderr copy goroutines share one writer, hence the mutex.
type probeLineWriter struct {
	stream *probeStream
	mu     sync.Mutex
	carry  []byte
}

func (w *probeLineWriter) Write(p []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.carry = append(w.carry, p...)
	for {
		end := bytes.IndexByte(w.carry, '\n')
		if end < 0 {
			break
		}
		line := w.carry[:end+1]
		w.carry = w.carry[end+1:]
		w.stream.chunk(decodeProbeOutput(line))
	}
	return len(p), nil
}

func (w *probeLineWriter) flush() {
	w.mu.Lock()
	defer w.mu.Unlock()
	if len(w.carry) > 0 {
		w.stream.chunk(decodeProbeOutput(w.carry))
		w.carry = nil
	}
}

// probeTTY gives the child a terminal. traceroute's libc output is fully
// buffered when stdout is a pipe, which made every hop arrive only after the
// process exited; a pty switches the child to per-line flushing.
type probeTTY struct {
	master, slave *os.File
}

func (t *probeTTY) pump(feed io.Writer) <-chan struct{} {
	done := make(chan struct{})
	go func() {
		defer close(done)
		buf := make([]byte, 4096)
		for {
			n, err := t.master.Read(buf)
			if n > 0 {
				_, _ = feed.Write(buf[:n])
			}
			if err != nil {
				return // EIO once every slave copy closed; the child exited.
			}
		}
	}()
	return done
}

func (t *probeTTY) closeSlave()  { _ = t.slave.Close() }
func (t *probeTTY) closeMaster() { _ = t.master.Close() }

// The system route-tracing tools already bound themselves: they stop after a
// fixed hop count, probing each hop a few times and waiting for each reply. A
// flat wall-clock cap cut off legitimate full-length traces (a filtered 20-hop
// path needs about two minutes), so the deadline is derived from exactly the
// hop and wait values handed to the tool, plus slack for name resolution and
// process startup. Ctrl+C still cancels immediately.
const (
	tracerouteMaximumHops  = 20
	tracerouteProbesPerHop = 3
	tracerouteProbeSlack   = 20 * time.Second
	traceroutePosixWait    = 2 * time.Second
	tracerouteWindowsWait  = 1 * time.Second
)

// tracerouteProbeCommand returns the fixed invocation and the matching deadline.
func tracerouteProbeCommand(host string) (string, []string, time.Duration) {
	hops := strconv.Itoa(tracerouteMaximumHops)
	if runtime.GOOS == "windows" {
		return "tracert", []string{"-d", "-h", hops, "-w", strconv.Itoa(int(tracerouteWindowsWait / time.Millisecond)), host}, tracerouteDeadline(tracerouteWindowsWait)
	}
	return "traceroute", []string{"-n", "-m", hops, "-w", strconv.Itoa(int(traceroutePosixWait / time.Second)), host}, tracerouteDeadline(traceroutePosixWait)
}

// tracerouteDeadline is the worst case in which every hop answers nothing, so
// every probe waits the full timeout.
func tracerouteDeadline(probeWait time.Duration) time.Duration {
	return time.Duration(tracerouteMaximumHops*tracerouteProbesPerHop)*probeWait + tracerouteProbeSlack
}

func tracerouteDeadlineNote(deadline time.Duration) string {
	return fmt.Sprintf("\n（已达到 %d 秒上限，提前停止）", int(deadline/time.Second))
}

func (a *App) registerLocalProbeHTTP(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/local-probe/nslookup", func(w http.ResponseWriter, r *http.Request) {
		host, ok := localProbeHost(w, r, "nslookup")
		if !ok {
			return
		}
		release, err := acquireLocalProbe()
		if err != nil {
			respond(w, nil, err)
			return
		}
		defer release()
		ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
		defer cancel()
		started := time.Now()
		var out strings.Builder
		fmt.Fprintf(&out, "本机 DNS → %s\n", host)
		if net.ParseIP(host) != nil {
			fmt.Fprintf(&out, "%s（本身即 IP 地址，无需解析）\n解析完成 · 用时 %dms", host, time.Since(started).Milliseconds())
			writeJSON(w, map[string]any{"output": out.String(), "ok": true})
			return
		}
		addrs, err := net.DefaultResolver.LookupIPAddr(ctx, host)
		if err != nil {
			respond(w, nil, fmt.Errorf("解析失败：%w", err))
			return
		}
		if len(addrs) == 0 {
			respond(w, nil, errors.New("该主机名没有可用地址记录"))
			return
		}
		for _, addr := range addrs {
			fmt.Fprintf(&out, "%s\n", addr.IP.String())
		}
		if cname, e := net.DefaultResolver.LookupCNAME(ctx, host); e == nil {
			cname = strings.TrimSuffix(cname, ".")
			if cname != "" && cname != host {
				fmt.Fprintf(&out, "规范名称：%s\n", cname)
			}
		}
		fmt.Fprintf(&out, "解析完成 · %d 条地址记录 · 用时 %dms", len(addrs), time.Since(started).Milliseconds())
		writeJSON(w, map[string]any{"output": out.String(), "ok": true})
	})
	mux.HandleFunc("POST /api/local-probe/dig", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Host string `json:"host"`
			Type string `json:"type"`
		}
		if !decode(w, r, &input) {
			return
		}
		host := strings.Trim(strings.TrimSpace(input.Host), "[]")
		if !validQuickHost(host) {
			respond(w, nil, errors.New("dig 后只接受主机名或 IP"))
			return
		}
		recordType := strings.ToUpper(strings.TrimSpace(input.Type))
		if recordType == "" {
			recordType = "A"
		}
		if !digRecordTypes[recordType] {
			respond(w, nil, errors.New("dig 支持 A / AAAA / CNAME / MX / TXT / NS 记录类型"))
			return
		}
		release, err := acquireLocalProbe()
		if err != nil {
			respond(w, nil, err)
			return
		}
		defer release()
		ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
		defer cancel()
		started := time.Now()
		var out strings.Builder
		fmt.Fprintf(&out, "; <<>> DevShell dig <<>> %s %s\n;; QUESTION SECTION:\n;%s.\t\tIN\t%s\n", host, recordType, host, recordType)
		answers := make([]string, 0, 8)
		switch recordType {
		case "A", "AAAA":
			if net.ParseIP(host) != nil {
				if (recordType == "A") == (net.ParseIP(host).To4() != nil) {
					answers = append(answers, fmt.Sprintf("%s.\tIN\t%s\t%s", host, recordType, host))
				}
			} else {
				addrs, lookupErr := net.DefaultResolver.LookupIPAddr(ctx, host)
				if lookupErr != nil {
					respond(w, nil, fmt.Errorf("查询失败：%w", lookupErr))
					return
				}
				for _, addr := range addrs {
					if (recordType == "A") == (addr.IP.To4() != nil) {
						answers = append(answers, fmt.Sprintf("%s.\tIN\t%s\t%s", host, recordType, addr.IP.String()))
					}
				}
			}
		case "CNAME":
			cname, lookupErr := net.DefaultResolver.LookupCNAME(ctx, host)
			if lookupErr != nil {
				respond(w, nil, fmt.Errorf("查询失败：%w", lookupErr))
				return
			}
			cname = strings.TrimSuffix(cname, ".")
			if cname != "" && cname != host {
				answers = append(answers, fmt.Sprintf("%s.\tIN\tCNAME\t%s.", host, cname))
			}
		case "MX":
			mx, lookupErr := net.DefaultResolver.LookupMX(ctx, host)
			if lookupErr != nil {
				respond(w, nil, fmt.Errorf("查询失败：%w", lookupErr))
				return
			}
			for _, record := range mx {
				answers = append(answers, fmt.Sprintf("%s.\tIN\tMX\t%d %s.", host, record.Pref, strings.TrimSuffix(record.Host, ".")))
			}
		case "TXT":
			texts, lookupErr := net.DefaultResolver.LookupTXT(ctx, host)
			if lookupErr != nil {
				respond(w, nil, fmt.Errorf("查询失败：%w", lookupErr))
				return
			}
			for _, text := range texts {
				answers = append(answers, fmt.Sprintf("%s.\tIN\tTXT\t%q", host, text))
			}
		case "NS":
			ns, lookupErr := net.DefaultResolver.LookupNS(ctx, host)
			if lookupErr != nil {
				respond(w, nil, fmt.Errorf("查询失败：%w", lookupErr))
				return
			}
			for _, record := range ns {
				answers = append(answers, fmt.Sprintf("%s.\tIN\tNS\t%s.", host, strings.TrimSuffix(record.Host, ".")))
			}
		}
		out.WriteString(";; ANSWER SECTION:\n")
		if len(answers) == 0 {
			out.WriteString("(无记录)")
		} else {
			out.WriteString(strings.Join(answers, "\n"))
		}
		fmt.Fprintf(&out, "\n\n;; 本机 DNS · 用时 %dms", time.Since(started).Milliseconds())
		writeJSON(w, map[string]any{"output": out.String(), "ok": true})
	})
	// tcping: repeated TCP connect attempts against host:port, timing each
	// handshake. Implemented in-app (Windows has no built-in tcping), and the
	// same endpoint drives the frontend's -t loop with count=1.
	mux.HandleFunc("POST /api/local-probe/tcping", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Host  string `json:"host"`
			Port  int    `json:"port"`
			Count int    `json:"count"`
			Brief bool   `json:"brief"`
		}
		if !decode(w, r, &input) {
			return
		}
		if !a.Appearance().TcpToolsEnabled {
			respond(w, nil, errors.New("tcping 与 telnet 已关闭，请在「设置 → 其他功能」开启"))
			return
		}
		host := strings.Trim(strings.TrimSpace(input.Host), "[]")
		if !validQuickHost(host) {
			respond(w, nil, errors.New("tcping 后只接受主机名或 IP"))
			return
		}
		if input.Port < 1 || input.Port > 65535 {
			respond(w, nil, errors.New("tcping 需要 1–65535 的端口（如 tcping 主机 80）"))
			return
		}
		count := input.Count
		if count <= 0 {
			count = 4
		}
		if count > 8 {
			count = 8
		}
		release, err := acquireLocalProbe()
		if err != nil {
			respond(w, nil, err)
			return
		}
		defer release()
		ctx, cancel := context.WithTimeout(r.Context(), time.Duration(count)*3*time.Second+5*time.Second)
		defer cancel()
		dialer := net.Dialer{Timeout: 3 * time.Second}
		address := net.JoinHostPort(host, strconv.Itoa(input.Port))
		var out strings.Builder
		if !input.Brief {
			resolved := host
			if net.ParseIP(host) == nil {
				if addrs, lookupErr := net.DefaultResolver.LookupIPAddr(ctx, host); lookupErr == nil && len(addrs) > 0 {
					resolved = addrs[0].IP.String()
				}
			}
			fmt.Fprintf(&out, "本机 TCP → %s (%s) 端口 %d\n", host, resolved, input.Port)
		}
		sent, connected := 0, 0
		for attempt := 1; attempt <= count; attempt++ {
			if ctx.Err() != nil {
				break
			}
			sent++
			began := time.Now()
			probe, dialErr := dialer.DialContext(ctx, "tcp", address)
			elapsed := time.Since(began).Milliseconds()
			if probe != nil {
				connected++
				_ = probe.Close()
			}
			if dialErr == nil {
				fmt.Fprintf(&out, "%d  已连接 · time=%dms\n", attempt, elapsed)
				continue
			}
			if ctx.Err() != nil {
				break // cancelled mid-dial by Ctrl+C: skip the final noise
			}
			fmt.Fprintf(&out, "%d  连接失败 · %s\n", attempt, tcpFailureNote(dialErr))
		}
		if !input.Brief {
			fmt.Fprintf(&out, "成功 %d / %d 次连接", connected, sent)
		}
		writeJSON(w, map[string]any{"output": strings.TrimRight(out.String(), "\n"), "ok": connected > 0})
	})
	mux.HandleFunc("POST /api/local-probe/traceroute", func(w http.ResponseWriter, r *http.Request) {
		host, ok := localProbeHost(w, r, "traceroute")
		if !ok {
			return
		}
		release, err := acquireLocalProbe()
		if err != nil {
			respond(w, nil, err)
			return
		}
		defer release()
		name, args, deadline := tracerouteProbeCommand(host)
		path, err := exec.LookPath(name)
		if err != nil {
			respond(w, nil, fmt.Errorf("本机未找到 %s 命令", name))
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), deadline)
		defer cancel()
		if r.Header.Get("X-DevShell-Probe-Stream") == "1" {
			if flusher, canFlush := w.(http.Flusher); canFlush {
				a.runTracerouteStream(ctx, w, flusher, name, path, args, deadline)
				return
			}
		}
		buffer := &limitedBuffer{limit: localProbeOutputLimit}
		command := exec.CommandContext(ctx, path, args...)
		hideProbeWindow(command)
		command.Stdout = buffer
		command.Stderr = buffer
		runErr := command.Run()
		output := strings.TrimRight(decodeProbeOutput(buffer.buf.Bytes()), "\r\n")
		if output == "" && runErr != nil {
			respond(w, nil, fmt.Errorf("%s 执行失败：%v", name, runErr))
			return
		}
		if ctx.Err() != nil {
			output += tracerouteDeadlineNote(deadline)
		}
		writeJSON(w, map[string]any{"output": output, "ok": runErr == nil})
	})
	mux.HandleFunc("POST /api/local-probe/curl", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			URL  string `json:"url"`
			Head bool   `json:"head"`
		}
		if !decode(w, r, &input) {
			return
		}
		target, err := parseLocalProbeURL(input.URL)
		if err != nil {
			respond(w, nil, err)
			return
		}
		release, err := acquireLocalProbe()
		if err != nil {
			respond(w, nil, err)
			return
		}
		defer release()
		ctx, cancel := context.WithTimeout(r.Context(), 20*time.Second)
		defer cancel()
		method := http.MethodGet
		if input.Head {
			method = http.MethodHead
		}
		request, err := http.NewRequestWithContext(ctx, method, target, nil)
		if err != nil {
			respond(w, nil, errors.New("网址无效"))
			return
		}
		request.Header.Set("User-Agent", "curl/8.0 (DevShell local prompt)")
		client := &http.Client{Timeout: 20 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
		response, err := client.Do(request)
		if err != nil {
			respond(w, nil, fmt.Errorf("请求失败：%v", err))
			return
		}
		defer response.Body.Close()
		var out strings.Builder
		fmt.Fprintf(&out, "%s %s\n", response.Proto, response.Status)
		response.Header.Write(&out)
		if !input.Head {
			out.WriteString("\r\n")
			body, readErr := io.ReadAll(io.LimitReader(response.Body, 64<<10))
			if readErr != nil {
				fmt.Fprintf(&out, "（读取响应体失败：%v）", readErr)
			} else {
				out.WriteString(strings.ToValidUTF8(string(body), "\uFFFD"))
				if len(body) == 64<<10 {
					out.WriteString("\n…（响应体已截断）")
				}
			}
		}
		writeJSON(w, map[string]any{"output": strings.TrimRight(out.String(), "\r\n"), "ok": response.StatusCode < 400})
	})
}

// runTracerouteStream keeps the frontend informed hop by hop. A pty is used
// where available so the tool line-buffers; otherwise plain pipes still work,
// just with output arriving whenever the child flushes.
func (a *App) runTracerouteStream(ctx context.Context, w http.ResponseWriter, flusher http.Flusher, name, path string, args []string, deadline time.Duration) {
	w.Header().Set("Content-Type", "application/x-ndjson")
	w.WriteHeader(http.StatusOK)
	stream := &probeStream{enc: json.NewEncoder(w), flusher: flusher}
	writer := &probeLineWriter{stream: stream}
	command := exec.CommandContext(ctx, path, args...)
	hideProbeWindow(command)
	tty, ttyErr := openLocalProbeTTY()
	if ttyErr == nil && tty != nil {
		command.Stdout = tty.slave
		command.Stderr = tty.slave
	} else {
		command.Stdout = writer
		command.Stderr = writer
	}
	var pumped <-chan struct{}
	if tty != nil {
		pumped = tty.pump(writer)
	}
	if err := command.Start(); err != nil {
		if tty != nil {
			tty.closeSlave()
			tty.closeMaster()
		}
		stream.chunk(fmt.Sprintf("%s 执行失败：%v", name, err))
		stream.done(false)
		return
	}
	if tty != nil {
		// The parent's slave copy must go: the master only sees EOF/EIO after
		// every remaining slave fd closes, i.e. when the child exits.
		tty.closeSlave()
	}
	runErr := command.Wait()
	if pumped != nil {
		<-pumped
		tty.closeMaster()
	}
	writer.flush()
	if ctx.Err() != nil {
		stream.chunk(tracerouteDeadlineNote(deadline))
	}
	stream.done(runErr == nil)
}

// parseLocalProbeURL mirrors the real curl tool: a scheme-less target such as
// "ip.sb" is accepted and defaults to http (curl.se URL syntax: prefix guesses
// like ftp. aside, "all other means HTTP"); explicit schemes must be http/https.
func parseLocalProbeURL(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" || len(raw) > 2048 || strings.ContainsAny(raw, " \t\r\n") {
		return "", errors.New("curl 后需要一个网址（不带协议时默认 http）")
	}
	if !strings.Contains(raw, "://") {
		raw = "http://" + raw
	}
	parsed, err := url.Parse(raw)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" {
		return "", errors.New("curl 仅支持 http/https 网址")
	}
	return parsed.String(), nil
}
