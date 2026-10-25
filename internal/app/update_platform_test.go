package app

import (
	"devshell/internal/updatetrust"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestLinuxUpdateFamilySelection(t *testing.T) {
	for _, tc := range []struct{ release, want string }{
		{"ID=arch", "pacman"},
		{"ID=manjaro\nID_LIKE=arch", "pacman"},
		{"ID=cachyos\nID_LIKE=\"arch\"", "pacman"},
		{"ID=ubuntu\nID_LIKE=debian", "deb"},
		{"ID=linuxmint\nID_LIKE=\"ubuntu debian\"", "deb"},
		{"ID=fedora\nID_LIKE=fedora", ""},
		{"ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"", ""},
		{"ID=unknown\nID_LIKE=\"arch debian\"", ""},
		{"ID=archlinux-fake", ""},
		{"", ""},
	} {
		if got := linuxPackageFormat(tc.release); got != tc.want {
			t.Errorf("%q: got %q, want %q", tc.release, got, tc.want)
		}
	}
}

func TestArchUpdateSignatureBindsPackageFamily(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	trusted := map[string]ed25519.PublicKey{updatetrust.KeyID(public): public}
	now := time.Now()
	for _, tc := range []struct {
		platform, want string
		tamper         bool
	}{
		{ArchUpdatePlatform, "available", false},
		{"linux-amd64", "none", false}, // Authentically signed DEB cannot become an Arch update.
		{"linux-amd64", "none", true},  // Relabelling DEB to pacman invalidates its signature.
	} {
		d := UpdateDescriptor{SchemaVersion: 2, Build: ApplicationBuild + 1, Product: "DevShell", Platform: tc.platform, Version: ApplicationVersion, SHA256: strings.Repeat("a", 64), ExecutableSHA256: strings.Repeat("b", 64), Size: 100}
		d, err = updatetrust.Sign(d, private, now, time.Hour)
		if err != nil {
			t.Fatal(err)
		}
		if tc.tamper {
			d.Platform = ArchUpdatePlatform
		}
		s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { json.NewEncoder(w).Encode(d) }))
		got := checkUpdateTrusted(context.Background(), s.Client(), s.URL, ArchUpdatePlatform, strings.Repeat("c", 64), UpdateReceipt{}, trusted, now)
		s.Close()
		if got.Status != tc.want {
			t.Fatalf("%+v: %+v", tc, got)
		}
		if got.Status == "available" && (got.Package.Format != "pacman" || got.Package.URL != "https://github.com/yuhongwei380/DevShell/releases/latest/download/up.pkg.tar.zst") {
			t.Fatalf("wrong Arch offer: %+v", got)
		}
	}
	manifest, artifact := updateAddress(ArchUpdatePlatform)
	if manifest != "https://github.com/yuhongwei380/DevShell/releases/latest/download/up.pkg.tar.zst.json" || artifact != "https://github.com/yuhongwei380/DevShell/releases/latest/download/up.pkg.tar.zst" {
		t.Fatal("Arch address changed")
	}
	if updatePackageFormat("linux-amd64") != "deb" || updatePackageFormat("windows-amd64") != "exe" {
		t.Fatal("legacy package formats changed")
	}
}

func TestMacOSUpdateOfferUsesUniversalBundle(t *testing.T) {
	now := time.Now()
	offer := func(platform, manifestPlatform string) UpdateStatus {
		descriptor := UpdateDescriptor{SchemaVersion: 2, Build: ApplicationBuild + 1, Product: "DevShell", Platform: manifestPlatform, Version: ApplicationVersion, Notes: "调整：测试", SHA256: strings.Repeat("a", 64), ExecutableSHA256: strings.Repeat("b", 64), Size: 100}
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { json.NewEncoder(w).Encode(descriptor) }))
		defer server.Close()
		return checkUpdateTrusted(context.Background(), server.Client(), server.URL, platform, strings.Repeat("c", 64), UpdateReceipt{}, updatetrust.PublisherKeys(), now)
	}
	for _, platform := range []string{DarwinArmUpdatePlatform, DarwinAmdUpdatePlatform} {
		manifest, artifact := updateAddress(platform)
		if !strings.HasSuffix(manifest, ".json") || !strings.HasPrefix(manifest, updateReleaseDownload) || artifact != updateReleaseDownload+macOSBundleAsset {
			t.Fatalf("%s address must point at the published universal bundle: %q %q", platform, manifest, artifact)
		}
		got := offer(platform, platform)
		if got.Status != "available" || got.Reason != "" || !got.InstallerReady {
			t.Fatalf("%s offer: %+v", platform, got)
		}
		if got.Package.Format != "app-zip" || got.Package.URL != updateReleaseDownload+macOSBundleAsset {
			t.Fatalf("wrong macOS offer: %+v", got.Package)
		}
	}
	// The universal archive is published twice on purpose: an Intel client must
	// not accept the Apple Silicon descriptor, and neither may an unknown host.
	if got := offer(DarwinArmUpdatePlatform, DarwinAmdUpdatePlatform); got.Status == "available" || got.Reason != "invalid" {
		t.Fatalf("cross-architecture descriptor accepted: %+v", got)
	}
	if updatePackageFormat(DarwinArmUpdatePlatform) != "app-zip" || updatePackageFormat(DarwinAmdUpdatePlatform) != "app-zip" {
		t.Fatal("macOS package format missing")
	}
	if updatePackageFormat("darwin-arm") != "" {
		t.Fatal("unknown platform gained a package format")
	}
	if manifest, artifact := updateAddress("darwin-arm"); manifest != "" || artifact != "" {
		t.Fatal("unknown platform gained update addresses")
	}
}
