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

func releaseTestAssets() []githubReleaseAsset {
	return []githubReleaseAsset{{Name: "up.exe.json", State: "uploaded"}, {Name: "up.exe", State: "uploaded"}}
}

func releaseListServer(list []githubRelease) *httptest.Server {
	data, _ := json.Marshal(list)
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}))
}

func TestParseUpdateReleaseTagAcceptsVersionOnlyTags(t *testing.T) {
	full := strconv.FormatUint(ApplicationBuild+1, 10)
	abbreviated := full[2:]
	for _, tc := range []struct {
		tag, version string
		build        uint64
		isRC, ok     bool
	}{
		{"v26.11.1-rc.7-build" + abbreviated, "v26.11.1", ApplicationBuild + 1, true, true},
		{"v26.11.1-rc.7-build" + full, "v26.11.1", ApplicationBuild + 1, true, true},
		{"v26.11.1-rc.7", "v26.11.1", 0, true, true},
		{"v26.11.1", "v26.11.1", 0, false, true},
		{"v26.11.1-rc.build" + abbreviated, "", 0, false, false},
		{"v26.11-build" + abbreviated, "", 0, false, false},
		{"v26.11.1-rc.7-build" + full[:9], "", 0, false, false},
		{"v26.11.1-rc.7-build261101", "", 0, false, false},
	} {
		version, build, isRC, ok := parseUpdateReleaseTag(tc.tag)
		if version != tc.version || build != tc.build || isRC != tc.isRC || ok != tc.ok {
			t.Fatalf("parseUpdateReleaseTag(%q) = %q %d rc=%v ok=%v; want %q %d rc=%v ok=%v", tc.tag, version, build, isRC, ok, tc.version, tc.build, tc.isRC, tc.ok)
		}
	}
}

func withLocalReleaseSources(t *testing.T, list []githubRelease, manifests map[string]uint64, manifestServed *int) (client *http.Client) {
	t.Helper()
	listServer := releaseListServer(list)
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
	oldList, oldBase := updateReleaseListURL, updateReleaseDownloadBase
	t.Cleanup(func() { updateReleaseListURL, updateReleaseDownloadBase = oldList, oldBase })
	updateReleaseListURL = listServer.URL + "/releases"
	updateReleaseDownloadBase = downloads.URL + "/download/"
	return &http.Client{}
}

func TestLatestReleaseUpdateSourceResolvesVersionOnlyTags(t *testing.T) {
	rc8, rc7 := ApplicationVersion+"-rc.8", ApplicationVersion+"-rc.7"
	manifests := map[string]uint64{rc8: ApplicationBuild + 2, rc7: ApplicationBuild + 1}
	served := 0
	client := withLocalReleaseSources(t, []githubRelease{
		{TagName: rc8, Prerelease: true, Assets: releaseTestAssets()},
		{TagName: rc7, Prerelease: true, Assets: releaseTestAssets()},
	}, manifests, &served)
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
	client := withLocalReleaseSources(t, []githubRelease{
		{TagName: rc7, Prerelease: true, Assets: releaseTestAssets()},
		{TagName: legacy, Prerelease: true, Assets: releaseTestAssets()},
	}, map[string]uint64{rc7: ApplicationBuild + 1}, nil)
	source, reason, _ := latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || !strings.HasSuffix(source.manifestURL, "/"+rc7+"/up.exe.json") {
		t.Fatalf("newer version-only release not chosen over legacy: %+v %q", source, reason)
	}

	client = withLocalReleaseSources(t, []githubRelease{
		{TagName: rc7, Prerelease: true, Assets: releaseTestAssets()},
		{TagName: legacy, Prerelease: true, Assets: releaseTestAssets()},
	}, map[string]uint64{rc7: ApplicationBuild - 1}, nil)
	source, reason, _ = latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || !strings.HasSuffix(source.manifestURL, "/"+legacy+"/up.exe.json") {
		t.Fatalf("stale version-only republish must not outrank newer legacy build: %+v %q", source, reason)
	}
}

func TestLatestReleaseUpdateSourceSkipsUnresolvableVersionOnlyTag(t *testing.T) {
	rc7 := ApplicationVersion + "-rc.7"
	legacy := ApplicationVersion + "-rc.6-build" + strconv.FormatUint(ApplicationBuild, 10)
	client := withLocalReleaseSources(t, []githubRelease{
		{TagName: rc7, Prerelease: true, Assets: releaseTestAssets()},
		{TagName: legacy, Prerelease: true, Assets: releaseTestAssets()},
	}, map[string]uint64{}, nil)
	source, reason, _ := latestReleaseUpdateSource(context.Background(), client, "windows-amd64", "")
	if reason != "" || !strings.HasSuffix(source.manifestURL, "/"+legacy+"/up.exe.json") {
		t.Fatalf("missing manifest did not skip the version-only candidate: %+v %q", source, reason)
	}
}

func TestLatestReleaseUpdateSourceResolvesBuildThroughMirror(t *testing.T) {
	rc8 := ApplicationVersion + "-rc.8"
	list := []githubRelease{{TagName: rc8, Prerelease: true, Assets: releaseTestAssets()}}
	listServer := releaseListServer(list)
	defer listServer.Close()
	directManifests := 0
	downloads := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		directManifests++
		http.NotFound(w, r)
	}))
	defer downloads.Close()
	mirror := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/"+listServer.URL+"/releases" {
			data, _ := json.Marshal(list)
			w.Header().Set("Content-Type", "application/json")
			w.Write(data)
			return
		}
		if r.URL.Path == "/"+downloads.URL+"/download/"+rc8+"/up.exe.json" {
			json.NewEncoder(w).Encode(UpdateDescriptor{SchemaVersion: 2, Build: ApplicationBuild + 2, Product: "DevShell", Platform: "windows-amd64"})
			return
		}
		http.NotFound(w, r)
	}))
	defer mirror.Close()
	oldList, oldBase := updateReleaseListURL, updateReleaseDownloadBase
	defer func() { updateReleaseListURL, updateReleaseDownloadBase = oldList, oldBase }()
	updateReleaseListURL = listServer.URL + "/releases"
	updateReleaseDownloadBase = downloads.URL + "/download/"
	source, reason, viaMirror := latestReleaseUpdateSource(context.Background(), &http.Client{}, "windows-amd64", mirror.URL)
	if reason != "" || !viaMirror {
		t.Fatalf("mirror release list rejected: %+v %q", source, reason)
	}
	if source.build != ApplicationBuild+2 || !strings.HasPrefix(source.manifestURL, updateReleaseDownloadBase) || directManifests != 0 {
		t.Fatalf("manifest not resolved through the mirror: %+v (direct calls %d)", source, directManifests)
	}
}
