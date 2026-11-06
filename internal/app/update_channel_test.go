package app

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

// The manual update check offers two channels: "release" stays on the
// stable /releases/latest assets, "rc" walks the full release list where
// prereleases live (which also lets a stable install opt into a newer rc
// publish). The empty channel keeps the shipped build's default discovery.
func TestUpdateChannelDiscoverySelection(t *testing.T) {
	if got := updateWantsReleaseList(""); got != isReleaseCandidateBuild() {
		t.Fatalf("default channel must mirror the shipped build kind: got %v", got)
	}
	if !updateWantsReleaseList("rc") {
		t.Fatal(`the "rc" channel must walk the release list even for stable builds`)
	}
	if updateWantsReleaseList("release") {
		t.Fatal(`the "release" channel must stay on the stable latest-release assets even for rc builds`)
	}
}

// Opting into the rc channel must satisfy the install-time ordering rules
// like every other update: a newer publish over the same functional version
// installs, an equal or older anti-rollback build stays rejected.
func TestUpdateChannelRcInstallOrdering(t *testing.T) {
	dir := t.TempDir()
	if err := ValidateUpdateBuild(dir, ApplicationBuild+1, ApplicationVersion); err != nil {
		t.Fatalf("a newer rc publish must be installable over a stable build: %v", err)
	}
	if err := ValidateUpdateBuild(dir, ApplicationBuild, ApplicationVersion); err == nil {
		t.Fatal("the current build must stay rejected")
	}
}

// The rc channel discovers prerelease publishes hidden from
// /releases/latest: a publish carrying only an rc tag must be offered.
func TestUpdateChannelRcFindsPrereleasePublish(t *testing.T) {
	platform := currentUpdatePlatform()
	if strings.HasPrefix(platform, "linux-") {
		t.Skip("Linux payloads are no longer published")
	}
	manifestName, artifactName := updateAssetNames(platform)
	if manifestName == "" {
		t.Skip("platform without built-in update assets")
	}
	oldList, oldBase := updateReleaseListURL, updateReleaseDownloadBase
	defer func() { updateReleaseListURL, updateReleaseDownloadBase = oldList, oldBase }()
	build := ApplicationBuild + 1
	tag := ApplicationVersion + "-rc.9-build" + strconv.FormatUint(build, 10)
	list, err := json.Marshal([]githubRelease{{TagName: tag, Prerelease: true, Assets: []githubReleaseAsset{{Name: manifestName, State: "uploaded"}, {Name: artifactName, State: "uploaded"}}}})
	if err != nil {
		t.Fatal(err)
	}
	manifest, err := json.Marshal(UpdateDescriptor{SchemaVersion: 2, Build: build, Product: "DevShell", Platform: platform, Version: ApplicationVersion, DisplayVersion: ApplicationVersion + "-rc.9", Notes: "rc 通道发现验证", SHA256: strings.Repeat("a", 64), ExecutableSHA256: strings.Repeat("a", 64), Size: 4096})
	if err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/releases":
			w.Header().Set("Content-Type", "application/json")
			w.Write(list)
		case "/" + tag + "/" + manifestName:
			w.Header().Set("Content-Type", "application/json")
			w.Write(manifest)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	updateReleaseListURL = server.URL + "/releases"
	updateReleaseDownloadBase = server.URL + "/"
	a, e := New(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	defer a.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	result := a.updateProbe(ctx, 5*time.Second, "rc")
	if result.Status != "available" || result.LatestBuild != build {
		t.Fatalf("rc channel must offer the prerelease publish: %+v", result)
	}
	if result.Package == nil || !strings.Contains(result.Package.URL, "/"+tag+"/") {
		t.Fatalf("rc offer must point at the prerelease publish assets: %+v", result.Package)
	}
}
