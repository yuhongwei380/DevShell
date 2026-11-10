package app

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// releaseFeedXML builds a minimal GitHub releases.atom payload: entries in
// publish order (newest first), each carrying the alternate link whose path
// holds the release tag — exactly what releaseFeedTags parses.
func releaseFeedXML(tags ...string) string {
	var b strings.Builder
	b.WriteString(`<?xml version="1.0" encoding="UTF-8"?>` + "\n")
	b.WriteString(`<feed xmlns="http://www.w3.org/2005/Atom">` + "\n")
	for _, tag := range tags {
		b.WriteString("<entry>\n")
		b.WriteString("<id>tag:github.com,2008:Repository/1424220324/" + tag + "</id>\n")
		b.WriteString(`<link rel="alternate" type="text/html" href="https://github.com/yuhongwei380/DevShell/releases/tag/` + tag + `"/>` + "\n")
		b.WriteString("<title>DevShell " + tag + "</title>\n")
		b.WriteString("</entry>\n")
	}
	b.WriteString("</feed>")
	return b.String()
}

func releaseFeedServer(tags []string) *httptest.Server {
	data := releaseFeedXML(tags...)
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/atom+xml")
		w.Write([]byte(data))
	}))
}

func TestParseUpdateReleaseTagAcceptsVersionOnlyTags(t *testing.T) {
	for _, tc := range []struct {
		tag           string
		version       string
		build         uint64
		prereleaseTag bool
	}{
		{"v26.10.24-rc.3", "v26.10.24", 0, true},
		{"v26.10.24-rc.11", "v26.10.24", 0, true},
		{"v26.10.26", "v26.10.26", 0, false},
	} {
		version, build, prereleaseTag, ok := parseUpdateReleaseTag(tc.tag)
		if !ok || version != tc.version || build != tc.build || prereleaseTag != tc.prereleaseTag {
			t.Fatalf("parseUpdateReleaseTag(%q) = (%q, %d, %v, %v); want (%q, %d, %v, true)", tc.tag, version, build, prereleaseTag, ok, tc.version, tc.build, tc.prereleaseTag)
		}
	}

	// Historical tag formats must keep decoding for installed clients.
	buildText := strconv.FormatUint(ApplicationBuild+1, 10)
	if version, build, prereleaseTag, ok := parseUpdateReleaseTag("v26.10.24-rc.3-build" + buildText); !ok || version != "v26.10.24" || build != ApplicationBuild+1 || !prereleaseTag {
		t.Fatalf("legacy tag with rc suffix no longer decodes: %q %d %v %v", version, build, prereleaseTag, ok)
	}
	if version, build, _, ok := parseUpdateReleaseTag("v26.10.24-build" + buildText); !ok || version != "v26.10.24" || build != ApplicationBuild+1 {
		t.Fatalf("legacy stable tag no longer decodes: %q %d %v", version, build, ok)
	}
	if _, _, _, ok := parseUpdateReleaseTag("v26.10.24-rc.3-build123"); ok {
		t.Fatal("malformed build suffix must be rejected")
	}
}

func TestReleaseFeedTagsParsesAlternateLinks(t *testing.T) {
	tags, err := releaseFeedTags([]byte(releaseFeedXML("v1.2.3-rc.4", "v1.2.3")))
	if err != nil || len(tags) != 2 || tags[0] != "v1.2.3-rc.4" || tags[1] != "v1.2.3" {
		t.Fatalf("feed tags parsed wrong: %q, %v", tags, err)
	}
	tags, err = releaseFeedTags([]byte(`<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><entry><id>tag:github.com,2008:Repository/1/x</id><link rel="enclosure" href="https://github.com/x/y/releases/tag/a/b"/></entry></feed>`))
	if err != nil || len(tags) != 0 {
		t.Fatalf("entry without a usable alternate link must be skipped, not guessed: %q, %v", tags, err)
	}
	if _, err := releaseFeedTags([]byte("<html>not a feed</html>")); err == nil {
		t.Fatal("non-feed payload must fail parsing")
	}
}

func withLocalReleaseSources(t *testing.T, tags []string, manifests map[string]uint64, manifestServed *int) *http.Client {
	t.Helper()
	listServer := releaseFeedServer(tags)
	t.Cleanup(listServer.Close)
	downloads := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		for tag, build := range manifests {
			if r.URL.Path == "/download/"+tag+"/up.exe.json" {
				if manifestServed != nil {
					*manifestServed++
				}
				json.NewEncoder(w).Encode(UpdateDescriptor{SchemaVersion: 2, Build: build, Product: "DevShell", Platform: "windows-amd64"})
				return
			}
		}
		http.NotFound(w, r)
	}))
	t.Cleanup(downloads.Close)
	oldFeed, oldBase := updateReleaseFeedURL, updateReleaseDownloadBase
	t.Cleanup(func() { updateReleaseFeedURL, updateReleaseDownloadBase = oldFeed, oldBase })
	updateReleaseFeedURL = listServer.URL + "/releases.atom"
	updateReleaseDownloadBase = downloads.URL + "/download/"
	return &http.Client{}
}

func TestLatestReleaseUpdateSourceResolvesVersionOnlyTags(t *testing.T) {
	rc8, rc7 := ApplicationVersion+"-rc.8", ApplicationVersion+"-rc.7"
	manifests := map[string]uint64{rc8: ApplicationBuild + 2, rc7: ApplicationBuild + 1}
	served := 0
	client := withLocalReleaseSources(t, []string{rc8, rc7}, manifests, &served)
	source, reason, viaMirror := latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || viaMirror {
		t.Fatalf("version-only tags rejected: %+v %q", source, reason)
	}
	if !strings.HasSuffix(source.manifestURL, "/"+rc8+"/up.exe.json") || source.build != ApplicationBuild+2 {
		t.Fatalf("wrong newest version-only release chosen: %+v", source)
	}
	if served != 1 {
		t.Fatalf("expected one manifest resolution per version, got %d", served)
	}
}

func TestLatestReleaseUpdateSourceOrdersMixedTagStyles(t *testing.T) {
	rc7 := ApplicationVersion + "-rc.7"
	legacy := ApplicationVersion + "-rc.6-build" + strconv.FormatUint(ApplicationBuild, 10)
	client := withLocalReleaseSources(t, []string{rc7, legacy}, map[string]uint64{rc7: ApplicationBuild + 1}, nil)
	source, reason, _ := latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || !strings.HasSuffix(source.manifestURL, "/"+rc7+"/up.exe.json") {
		t.Fatalf("newer version-only release not chosen over legacy: %+v %q", source, reason)
	}

	client = withLocalReleaseSources(t, []string{rc7, legacy}, map[string]uint64{rc7: ApplicationBuild - 1}, nil)
	source, reason, _ = latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || !strings.HasSuffix(source.manifestURL, "/"+legacy+"/up.exe.json") {
		t.Fatalf("stale version-only republish must not outrank newer legacy build: %+v %q", source, reason)
	}
}

func TestLatestReleaseUpdateSourceSkipsUnresolvableVersionOnlyTag(t *testing.T) {
	rc7 := ApplicationVersion + "-rc.7"
	legacy := ApplicationVersion + "-rc.6-build" + strconv.FormatUint(ApplicationBuild, 10)
	client := withLocalReleaseSources(t, []string{rc7, legacy}, map[string]uint64{}, nil)
	source, reason, _ := latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || !strings.HasSuffix(source.manifestURL, "/"+legacy+"/up.exe.json") {
		t.Fatalf("missing manifest did not skip the version-only candidate: %+v %q", source, reason)
	}
}

func TestLatestReleaseUpdateSourceResolvesBuildThroughMirror(t *testing.T) {
	rc8 := ApplicationVersion + "-rc.8"
	listServer := releaseFeedServer([]string{rc8})
	defer listServer.Close()
	directManifests := 0
	downloads := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		directManifests++
		http.NotFound(w, r)
	}))
	defer downloads.Close()
	mirror := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/"+listServer.URL+"/releases.atom" {
			w.Header().Set("Content-Type", "application/atom+xml")
			w.Write([]byte(releaseFeedXML(rc8)))
			return
		}
		if r.URL.Path == "/"+downloads.URL+"/download/"+rc8+"/up.exe.json" {
			json.NewEncoder(w).Encode(UpdateDescriptor{SchemaVersion: 2, Build: ApplicationBuild + 2, Product: "DevShell", Platform: "windows-amd64"})
			return
		}
		http.NotFound(w, r)
	}))
	defer mirror.Close()
	oldFeed, oldBase := updateReleaseFeedURL, updateReleaseDownloadBase
	defer func() { updateReleaseFeedURL, updateReleaseDownloadBase = oldFeed, oldBase }()
	updateReleaseFeedURL = listServer.URL + "/releases.atom"
	updateReleaseDownloadBase = downloads.URL + "/download/"
	source, reason, viaMirror := latestReleaseUpdateSource(context.Background(), &http.Client{}, "windows-amd64", mirror.URL)
	if reason != "" || !viaMirror {
		t.Fatalf("mirror release feed rejected: %+v %q", source, reason)
	}
	if source.build != ApplicationBuild+2 || !strings.HasPrefix(source.manifestURL, updateReleaseDownloadBase) || directManifests != 0 {
		t.Fatalf("manifest not resolved through the mirror: %+v (direct calls %d)", source, directManifests)
	}
}
