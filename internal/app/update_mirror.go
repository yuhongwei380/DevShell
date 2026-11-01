package app

import (
	"errors"
	"net/url"
	"strings"
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
