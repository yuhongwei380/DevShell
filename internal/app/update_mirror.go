package app

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// normalizeUpdateMirror validates and normalizes the optional GitHub
// accelerator origin used for update traffic (the ghproxy-style URL-prefix
// convention used by mirrors such as gh-proxy.org). An empty value restores
// direct connections. Bare hosts and https origins are accepted and stored as
// "https://host" without a trailing slash; paths, credentials and queries are
// rejected so the prefixing behaviour below stays predictable.
func normalizeUpdateMirror(value string) (string, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return "", nil
	}
	if len(value) > 256 || strings.ContainsAny(value, "\x00 \t\r\n") {
		return "", errors.New("加速域名仅支持 https 主机名，例如 https://gh-proxy.org")
	}
	if !strings.Contains(value, "://") {
		value = "https://" + value
	}
	parsed, err := url.Parse(value)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || (parsed.Path != "" && parsed.Path != "/") {
		return "", errors.New("加速域名仅支持 https 主机名，例如 https://gh-proxy.org")
	}
	return "https://" + strings.ToLower(parsed.Host), nil
}

// updateMirrorHost returns the lowercased host used in the update client's
// redirect allowlist, or "" when no mirror is configured.
func updateMirrorHost(mirror string) string {
	if mirror == "" {
		return ""
	}
	if parsed, err := url.Parse(mirror); err == nil {
		return strings.ToLower(parsed.Hostname())
	}
	return ""
}

// applyUpdateMirror prefixes a canonical update URL for the accelerator:
// https://mirror.example/https://github.com/... — the shared convention of
// ghproxy-style public mirrors.
func applyUpdateMirror(mirror, target string) string {
	if mirror == "" {
		return target
	}
	return mirror + "/" + target
}

// updateFetchAttempts orders update fetch candidates: with a mirror
// configured the accelerator is tried first and the canonical GitHub URL
// remains as the fallback, so a broken accelerator degrades to a direct
// connection instead of failing the update. Without a mirror it is direct
// only.
func updateFetchAttempts(mirror, target string) []string {
	if mirror == "" {
		return []string{target}
	}
	return []string{applyUpdateMirror(mirror, target), target}
}

// UpdateMirrorProbe reports one connectivity probe of the accelerator (or a
// direct connection) against a URL the update flow would actually fetch:
// the discovery feed ("list") and the stable-channel manifest ("download").
// A 200 whose body does not carry the expected signature (mirror error
// pages) is reported as a failure rather than success.
type UpdateMirrorProbe struct {
	Role      string `json:"role"`
	OK        bool   `json:"ok"`
	Status    int    `json:"status"`
	LatencyMS int64  `json:"latencyMS"`
	Reason    string `json:"reason,omitempty"`
}

type UpdateMirrorTest struct {
	Host   string              `json:"host"`
	Probes []UpdateMirrorProbe `json:"probes"`
}

// testUpdateMirrorAt probes the mirror with the same client the update flow
// uses (same proxy handling and redirect allowlist), so the result reflects
// what a real check would experience. The two targets match the real
// channels: the discovery feed only the rc channel fetches, and the stable
// /releases/latest manifest that the release channel has always used (that
// channel itself is unchanged by the feed switch).
func testUpdateMirrorAt(ctx context.Context, client *http.Client, mirror, platform string) UpdateMirrorTest {
	feedTarget := updateReleaseFeedURL
	manifestTarget, _ := updateAddress(platform)
	targets := []struct{ role, url, signature string }{
		{"list", applyUpdateMirror(mirror, feedTarget), "<feed"},
		{"download", applyUpdateMirror(mirror, manifestTarget), `{"`},
	}
	test := UpdateMirrorTest{Host: updateMirrorHost(mirror)}
	for _, target := range targets {
		probe := UpdateMirrorProbe{Role: target.role}
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, target.url, nil)
		if err != nil {
			probe.Reason = "invalid-url"
			test.Probes = append(test.Probes, probe)
			continue
		}
		req.Header.Set("User-Agent", "DevShell/"+strings.TrimPrefix(ApplicationVersion, "v"))
		req.Header.Set("Cache-Control", "no-cache")
		started := time.Now()
		response, err := client.Do(req)
		if err != nil {
			probe.LatencyMS = time.Since(started).Milliseconds()
			probe.Reason = "network"
			test.Probes = append(test.Probes, probe)
			continue
		}
		data, _ := io.ReadAll(io.LimitReader(response.Body, 8193))
		response.Body.Close()
		probe.LatencyMS = time.Since(started).Milliseconds()
		probe.Status = response.StatusCode
		switch {
		case response.StatusCode != http.StatusOK:
			probe.Reason = "status"
		case !strings.Contains(string(data), target.signature):
			probe.Reason = "unexpected-body"
		default:
			probe.OK = true
		}
		test.Probes = append(test.Probes, probe)
	}
	return test
}
