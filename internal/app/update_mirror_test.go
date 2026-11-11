package app

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"testing"
	"time"

	"devshell/internal/updatetrust"
)

func TestNormalizeUpdateMirror(t *testing.T) {
	for _, tc := range []struct{ name, input, want string }{
		{"empty restores direct", "", ""},
		{"preset origin", "https://gh-proxy.org", "https://gh-proxy.org"},
		{"bare host", "ghfast.top", "https://ghfast.top"},
		{"trailing slash trimmed", "https://mirror.mikus.ink/", "https://mirror.mikus.ink"},
		{"host lowercased", "https://GHProxy.ORG", "https://ghproxy.org"},
		{"explicit port kept", "https://mirror.example:8443", "https://mirror.example:8443"},
		{"surrounding spaces trimmed", "  https://ghfast.top  ", "https://ghfast.top"},
	} {
		got, err := normalizeUpdateMirror(tc.input)
		if err != nil || got != tc.want {
			t.Fatalf("%s: normalizeUpdateMirror(%q) = %q, %v; want %q", tc.name, tc.input, got, err, tc.want)
		}
	}
	for _, bad := range []string{"http://gh-proxy.org", "https://gh-proxy.org/proxy", "https://user:pw@gh-proxy.org", "https://gh-proxy.org/?a=b", "https://gh-proxy.org/#x", "https://", "gh proxy.example", "https://gh-proxy.org\n/../"} {
		if got, err := normalizeUpdateMirror(bad); err == nil {
			t.Fatalf("normalizeUpdateMirror(%q) wrongly accepted as %q", bad, got)
		}
	}
}

func TestUpdateMirrorAppliesGhproxyPrefixAndOrder(t *testing.T) {
	if got := applyUpdateMirror("", "https://github.com/x/up.exe"); got != "https://github.com/x/up.exe" {
		t.Fatalf("direct mirror changed URL: %q", got)
	}
	if got := applyUpdateMirror("https://ghfast.top", "https://github.com/x/up.exe"); got != "https://ghfast.top/https://github.com/x/up.exe" {
		t.Fatalf("ghproxy-style prefix wrong: %q", got)
	}
	if attempts := updateFetchAttempts("https://gh-proxy.org", "https://github.com/x/up.exe.json"); len(attempts) != 2 || attempts[0] != "https://gh-proxy.org/https://github.com/x/up.exe.json" || attempts[1] != "https://github.com/x/up.exe.json" {
		t.Fatalf("mirror-first ordering wrong: %v", attempts)
	}
	if attempts := updateFetchAttempts("", "https://github.com/x"); len(attempts) != 1 || attempts[0] != "https://github.com/x" {
		t.Fatalf("direct attempts must stay single: %v", attempts)
	}
}

func TestUpdateRedirectPolicyOnlyAddsConfiguredMirror(t *testing.T) {
	parse := func(raw string) *url.URL {
		t.Helper()
		u, err := url.Parse(raw)
		if err != nil {
			t.Fatal(err)
		}
		return u
	}
	via := []*http.Request{{URL: parse("https://github.com/yuhongwei380/DevShell/releases/download/x/up.exe")}}
	mirrorClient := updateClient(5*time.Second, "https://ghfast.top")
	if err := mirrorClient.CheckRedirect(&http.Request{URL: parse("https://ghfast.top/https://github.com/x/up.exe")}, via); err != nil {
		t.Fatalf("configured mirror redirect rejected: %v", err)
	}
	if err := mirrorClient.CheckRedirect(&http.Request{URL: parse("https://evil.example/up.exe")}, via); err == nil {
		t.Fatal("foreign host accepted even with mirror configured")
	}
	directClient := updateClient(5*time.Second, "")
	if err := directClient.CheckRedirect(&http.Request{URL: parse("https://ghfast.top/https://github.com/x/up.exe")}, via); err == nil {
		t.Fatal("mirror host accepted while no mirror is configured")
	}
}

func TestUpdateManifestPrefersMirrorThenFallsBackToDirect(t *testing.T) {
	descriptor := UpdateDescriptor{SchemaVersion: 2, Build: ApplicationBuild + 1, Product: "DevShell", Platform: "linux-amd64", Version: ApplicationVersion, Notes: "via mirror", SHA256: strings.Repeat("a", 64), ExecutableSHA256: strings.Repeat("b", 64), Size: 100}
	directCalls, mirrorSucceeds := 0, true
	direct := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		directCalls++
		json.NewEncoder(w).Encode(descriptor)
	}))
	defer direct.Close()
	mirror := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if mirrorSucceeds && strings.HasPrefix(r.URL.Path, "/"+direct.URL+"/") {
			json.NewEncoder(w).Encode(descriptor)
			return
		}
		http.Error(w, "nope", http.StatusInternalServerError)
	}))
	defer mirror.Close()
	client := &http.Client{Timeout: 5 * time.Second}
	address := direct.URL + "/up.exe.json"

	got := checkUpdateTrustedAt(context.Background(), client, address, direct.URL+"/up.exe", "linux-amd64", strings.Repeat("c", 64), UpdateReceipt{}, updatetrust.PublisherKeys(), time.Now(), mirror.URL)
	if got.Status != "available" || got.Mirror != updateMirrorHost(mirror.URL) || directCalls != 0 {
		t.Fatalf("mirror-first manifest fetch wrong: %+v (direct calls %d)", got, directCalls)
	}
	if got.Package == nil || got.Package.URL != direct.URL+"/up.exe" {
		t.Fatalf("artifact URL must stay canonical GitHub, got %+v", got.Package)
	}

	mirrorSucceeds = false
	got = checkUpdateTrustedAt(context.Background(), client, address, direct.URL+"/up.exe", "linux-amd64", strings.Repeat("c", 64), UpdateReceipt{}, updatetrust.PublisherKeys(), time.Now(), mirror.URL)
	if got.Status != "available" || got.Mirror != "" || directCalls != 1 {
		t.Fatalf("broken mirror did not fall back to direct: %+v (direct calls %d)", got, directCalls)
	}

	got = checkUpdateTrustedAt(context.Background(), client, address, direct.URL+"/up.exe", "linux-amd64", strings.Repeat("c", 64), UpdateReceipt{}, updatetrust.PublisherKeys(), time.Now(), "")
	if got.Status != "available" || got.Mirror != "" || directCalls != 2 {
		t.Fatalf("direct-only check regressed: %+v (direct calls %d)", got, directCalls)
	}
}

func TestLatestReleaseFeedGoesThroughMirrorWithDirectFallback(t *testing.T) {
	old := updateReleaseFeedURL
	defer func() { updateReleaseFeedURL = old }()
	buildText := strconv.FormatUint(ApplicationBuild+1, 10)
	feed := releaseFeedXML(ApplicationVersion + "-rc.9-build" + buildText)
	mirrorSucceeds := true
	mirror := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if mirrorSucceeds && strings.HasPrefix(r.URL.Path, "/https://github.com/") {
			w.Header().Set("Content-Type", "application/atom+xml")
			w.Write([]byte(feed))
			return
		}
		http.NotFound(w, r)
	}))
	defer mirror.Close()
	direct := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/atom+xml")
		w.Write([]byte(feed))
	}))
	defer direct.Close()
	client := &http.Client{Timeout: 5 * time.Second}

	updateReleaseFeedURL = "https://github.com/tests/releases.atom"
	source, reason, viaMirror := latestReleaseUpdateSource(context.Background(), client, "windows-amd64", mirror.URL)
	if reason != "" || !viaMirror {
		t.Fatalf("release feed not served via accelerator mirror: %+v %q", source, reason)
	}
	if !strings.Contains(source.manifestURL, "/"+ApplicationVersion+"-rc.9-build"+buildText+"/up.exe.json") || !strings.HasPrefix(source.manifestURL, updateReleaseDownloadBase) {
		t.Fatalf("download source wrong: %+v", source)
	}

	updateReleaseFeedURL = direct.URL + "/releases.atom"
	mirrorSucceeds = false
	source, reason, viaMirror = latestReleaseUpdateSource(context.Background(), client, "windows-amd64", mirror.URL)
	if reason != "" || viaMirror {
		t.Fatalf("mirror failure did not fall back to the direct release feed: %+v %q", source, reason)
	}
}

func TestUpdateMirrorPersistsNormalizedThroughPatch(t *testing.T) {
	s, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	saved, err := s.PatchAppearance(preferencePatch(t, `{"updateMirror":"mirror.mikus.ink"}`))
	if err != nil {
		t.Fatal(err)
	}
	if saved.UpdateMirror != "https://mirror.mikus.ink" || s.UpdateMirror() != "https://mirror.mikus.ink" {
		t.Fatalf("mirror not normalized/persisted: %+v", saved)
	}
	if _, err = s.PatchAppearance(preferencePatch(t, `{"updateMirror":"http://gh-proxy.org"}`)); err == nil || !strings.Contains(err.Error(), "加速域名") {
		t.Fatalf("insecure mirror accepted without error: %v", err)
	}
	saved, err = s.PatchAppearance(preferencePatch(t, `{"updateMirror":""}`))
	if err != nil || saved.UpdateMirror != "" || s.UpdateMirror() != "" {
		t.Fatalf("clearing the mirror failed: %+v %v", saved, err)
	}
	if s.UpdateMirror() != "" {
		t.Fatal("store leaked a mirror after clearing")
	}
}

func TestTestUpdateMirrorAtReportsPerRoleHealth(t *testing.T) {
	platform := currentUpdatePlatform()
	manifestName, _ := updateAssetNames(platform)
	if manifestName == "" {
		t.Skip("platform without built-in update assets")
	}
	oldFeed, oldDownload := updateReleaseFeedURL, updateReleaseDownload
	defer func() { updateReleaseFeedURL, updateReleaseDownload = oldFeed, oldDownload }()
	feedXML := releaseFeedXML("v9.9.9")
	hub := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/feed":
			w.Header().Set("Content-Type", "application/atom+xml")
			w.Write([]byte(feedXML))
		case "/dl/" + manifestName:
			w.Header().Set("Content-Type", "application/json")
			w.Write([]byte("{\n  \"schemaVersion\": 2,\n  \"build\": 42\n}"))
		default:
			http.NotFound(w, r)
		}
	}))
	defer hub.Close()
	mode := "healthy"
	mirror := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/"+updateReleaseFeedURL {
			switch mode {
			case "healthy":
				w.Header().Set("Content-Type", "application/atom+xml")
				w.Write([]byte(feedXML))
			case "no-list":
				http.Error(w, "forbidden", http.StatusForbidden)
			default:
				w.Write([]byte("<html>blocked by origin allowlist</html>"))
			}
			return
		}
		if r.URL.Path == "/"+updateReleaseDownload+manifestName {
			switch mode {
			case "garbage":
				w.Write([]byte("<html>blocked by origin allowlist</html>"))
			default:
				w.Header().Set("Content-Type", "application/json")
				w.Write([]byte("{\n  \"schemaVersion\": 2,\n  \"build\": 42\n}"))
			}
			return
		}
		http.NotFound(w, r)
	}))
	defer mirror.Close()
	updateReleaseFeedURL = "https://example.invalid/feed"
	updateReleaseDownload = "https://example.invalid/dl/"
	client := updateClient(5*time.Second, mirror.URL)

	byRole := func(test UpdateMirrorTest) map[string]UpdateMirrorProbe {
		probes := map[string]UpdateMirrorProbe{}
		for _, probe := range test.Probes {
			probes[probe.Role] = probe
		}
		return probes
	}
	test := testUpdateMirrorAt(context.Background(), client, mirror.URL, platform)
	probes := byRole(test)
	if test.Host != "127.0.0.1" || len(probes) != 2 {
		t.Fatalf("healthy mirror probes wrong: %+v", test)
	}
	if !probes["list"].OK || !probes["download"].OK {
		t.Fatalf("healthy mirror reported failures: %+v", test.Probes)
	}

	mode = "no-list"
	probes = byRole(testUpdateMirrorAt(context.Background(), client, mirror.URL, platform))
	if probes["list"].OK || probes["list"].Reason != "status" || probes["list"].Status != http.StatusForbidden {
		t.Fatalf("mirror without API-style proxying misreported: %+v", probes["list"])
	}
	if !probes["download"].OK {
		t.Fatalf("working download role misreported: %+v", probes["download"])
	}

	mode = "garbage"
	probes = byRole(testUpdateMirrorAt(context.Background(), client, mirror.URL, platform))
	if probes["list"].OK || probes["list"].Reason != "unexpected-body" || probes["download"].OK || probes["download"].Reason != "unexpected-body" {
		t.Fatalf("200-with-HTML bodies must not count as connectivity: %+v", test.Probes)
	}

	updateReleaseFeedURL = hub.URL + "/feed"
	updateReleaseDownload = hub.URL + "/dl/"
	test = testUpdateMirrorAt(context.Background(), updateClient(5*time.Second, ""), "", platform)
	probes = byRole(test)
	if test.Host != "" || !probes["list"].OK || !probes["download"].OK {
		t.Fatalf("direct probes wrong: %+v", test)
	}
}
