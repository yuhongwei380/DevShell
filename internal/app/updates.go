package app

import (
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"devshell/internal/updateproxy"
	"devshell/internal/updatetrust"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Version rule for this branch: v<X>.<Y>.<Z> — X is the two-digit year, Y the
// month without a leading zero, Z the zero-based release index within that
// month. The first release of October 2026 is v26.10.0. Test (rc) builds ship
// under this same functional version because update comparison parses numeric
// segments only; the rc.N label lives in ApplicationDisplayVersion, the tag
// and the notes heading (vX.Y.Z-rc.N-build<abbreviated build>). rc build
// numbers keep ascending, and the eventual release build must exceed every
// rc build of the same version so rc users can update into it.
const ApplicationVersion = "v26.10.4"

// ApplicationDisplayVersion identifies this self-maintained branch in the user
// interface. ApplicationVersion above stays the functional version that drives
// update comparison and package metadata; never put branch text into it.
const ApplicationDisplayVersion = "ver.Phoenix · v26.10.4-rc.2"

// Package managers use ApplicationVersion without the leading v. It must stay
// greater than the last upstream package version (0.3.0).
const ApplicationPackageVersion = "26.10.4"

// ApplicationBuild is the monotonic anti-rollback build: YYYYMMDD plus a
// two-digit daily sequence (since 2026-10-24; the earlier format was a
// three-digit sequence). Increase it for every published build, including
// packaging-only releases; update manifests must exceed it. The 10-digit
// sequence is numerically below the retired 11-digit high-water mark
// (20261024156), so installs at any older build must manually install the
// 2026102401 release once before built-in updates continue normally.
const ApplicationBuild uint64 = 2026102801

// Updates are published as GitHub Release assets of the maintained repository.
// Release assets must be named exactly: up.exe / up.exe.json,
// up.deb / up.deb.json, up.pkg.tar.zst / up.pkg.tar.zst.json,
// DevShell-macos-universal.zip with up-macos-arm64.json / up-macos-amd64.json.
const updateReleaseDownload = "https://github.com/yuhongwei380/DevShell/releases/latest/download/"
const UpdateManifestURL = updateReleaseDownload + "up.deb.json"

// macOSBundleAsset is the universal application bundle archive built by
// scripts/build/build-macos.py; both architecture descriptors point at it.
const macOSBundleAsset = "DevShell-macos-universal.zip"
const updateTimeout = 3 * time.Second

// Manual checks wait on the network in front of the user, unlike the
// non-blocking startup probe, so they get a longer budget.
const manualUpdateTimeout = 10 * time.Second
const maximumUpdateSize int64 = 1 << 30

var updateHashPattern = regexp.MustCompile(`^[a-fA-F0-9]{64}$`)

type UpdateDescriptor = updatetrust.Descriptor
type UpdatePackage struct {
	Format           string `json:"format"`
	Build            uint64 `json:"build"`
	Version          string `json:"version"`
	URL              string `json:"url"`
	SHA256           string `json:"sha256"`
	Size             int64  `json:"size"`
	ExecutableSHA256 string `json:"executableSHA256"`
	expiresAt        int64
}
type UpdateStatus struct {
	Status         string         `json:"status"`
	CurrentVersion string         `json:"currentVersion"`
	CurrentBuild   uint64         `json:"currentBuild"`
	LatestVersion  string         `json:"latestVersion,omitempty"`
	LatestBuild    uint64         `json:"latestBuild,omitempty"`
	Notes          string         `json:"notes,omitempty"`
	Source         string         `json:"source"`
	Platform       string         `json:"platform"`
	InstallerReady bool           `json:"installerReady"`
	Reason         string         `json:"reason,omitempty"`
	Package        *UpdatePackage `json:"package,omitempty"`
}
type UpdateReceipt struct {
	Build            uint64 `json:"build"`
	Version          string `json:"version"`
	SHA256           string `json:"sha256"`
	ExecutableSHA256 string `json:"executableSHA256"`
	InstalledAt      string `json:"installedAt"`
}

// updateLegacyBuildFloor marks the build-number format rebase (2026-10-24):
// new builds use YYYYMMDD plus a two-digit daily sequence (10 digits), while
// the historical full format carried a three-digit sequence (11 digits, for
// example 20261024156). A persisted high-water mark must never cross the
// rebase: an 11-digit receipt out-ranks every 10-digit build and would pin
// the client into a permanent build-rollback rejection ("already current").
const updateLegacyBuildFloor = 10_000_000_000

// receiptAppliesToCurrentBuild reports whether the receipt's build high-water
// is comparable with the compiled ApplicationBuild. A receipt written under
// the other build-number format is stale and no longer gates updates.
func receiptAppliesToCurrentBuild(receiptBuild uint64) bool {
	if receiptBuild == 0 {
		return true
	}
	return (receiptBuild >= updateLegacyBuildFloor) == (ApplicationBuild >= updateLegacyBuildFloor)
}

type UpdateDownload struct {
	ID           string        `json:"id"`
	Status       string        `json:"status"`
	Received     int64         `json:"received"`
	Total        int64         `json:"total"`
	Error        string        `json:"error,omitempty"`
	File         string        `json:"-"`
	Package      UpdatePackage `json:"-"`
	cancel       context.CancelFunc
	releaseLease func()
}
type startupUpdate struct {
	once     sync.Once
	done     chan struct{}
	result   UpdateStatus
	mu       sync.Mutex
	job      *UpdateDownload
	manualMu sync.Mutex
}

func updateAddress(platform string) (string, string) {
	manifest, artifact := updateAssetNames(platform)
	if manifest == "" {
		return "", ""
	}
	return updateReleaseDownload + manifest, updateReleaseDownload + artifact
}

func updateAssetNames(platform string) (string, string) {
	switch platform {
	case "windows-amd64":
		return "up.exe.json", "up.exe"
	case ArchUpdatePlatform:
		return "up.pkg.tar.zst.json", "up.pkg.tar.zst"
	case "linux-amd64":
		return "up.deb.json", "up.deb"
	case DarwinArmUpdatePlatform:
		return "up-macos-arm64.json", macOSBundleAsset
	case DarwinAmdUpdatePlatform:
		return "up-macos-amd64.json", macOSBundleAsset
	default:
		return "", ""
	}
}

var updateTransport = func() *http.Transport {
	t := http.DefaultTransport.(*http.Transport).Clone()
	t.Proxy = updateproxy.Proxy
	return t
}()

func updateClient(timeout time.Duration) *http.Client {
	return &http.Client{Transport: updateTransport, Timeout: timeout, CheckRedirect: func(req *http.Request, via []*http.Request) error {
		// GitHub serves release assets from objects.githubusercontent.com and,
		// since the 2025 asset migration, release-assets.githubusercontent.com.
		// Both are official GitHub asset hosts; anything else is rejected.
		if len(via) > 2 || req.URL.Scheme != "https" || (req.URL.Host != "api.github.com" && req.URL.Host != "github.com" && req.URL.Host != "objects.githubusercontent.com" && req.URL.Host != "release-assets.githubusercontent.com") || req.URL.User != nil {
			return errors.New("更新地址跳转无效")
		}
		return nil
	}}
}
func noUpdate(reason string) UpdateStatus {
	platform := currentUpdatePlatform()
	source, _ := updateAddress(platform)
	if isReleaseCandidateBuild() {
		source = updateReleaseListURL
	}
	return UpdateStatus{Status: "none", CurrentVersion: ApplicationDisplayVersion, CurrentBuild: ApplicationBuild, Source: source, Platform: platform, Reason: reason}
}
func FileSHA256(path string) (string, error) {
	f, e := os.Open(path)
	if e != nil {
		return "", e
	}
	defer f.Close()
	h := sha256.New()
	if _, e = io.Copy(h, f); e != nil {
		return "", e
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
func (a *App) ConfigDirectory() string { return a.store.dir }
func (a *App) beginUpdateCheck() {
	a.updateCheck.once.Do(func() {
		a.updateCheck.done = make(chan struct{})
		go func() {
			defer close(a.updateCheck.done)
			ctx, cancel := context.WithTimeout(a.ctx, updateTimeout)
			defer cancel()
			a.setUpdateResult(boundedUpdateProbe(ctx, func() UpdateStatus { return a.updateProbe(ctx, updateTimeout) }))
		}()
	})
}

// The manual check refreshes the cached offer so a release published after
// startup can be downloaded in the same session; the download compares the
// hash the user just confirmed against this same offer.
func (a *App) setUpdateResult(result UpdateStatus) {
	a.updateCheck.mu.Lock()
	a.updateCheck.result = result
	a.updateCheck.mu.Unlock()
}
func (a *App) getUpdateResult() UpdateStatus {
	a.updateCheck.mu.Lock()
	defer a.updateCheck.mu.Unlock()
	return a.updateCheck.result
}

// updateProbe runs one end-to-end update check; the manual settings action
// reuses it with a longer budget than the non-blocking startup probe.
func (a *App) updateProbe(ctx context.Context, timeout time.Duration) UpdateStatus {
	exe, err := os.Executable()
	if err != nil {
		return noUpdate("unavailable")
	}
	hash, err := FileSHA256(exe)
	if err != nil {
		return noUpdate("unavailable")
	}
	receipt, err := ReadUpdateReceipt(a.store.dir)
	if err != nil {
		return noUpdate("invalid-receipt")
	}
	platform := currentUpdatePlatform()
	address, artifact := updateAddress(platform)
	client := updateClient(timeout)
	if isReleaseCandidateBuild() {
		source, reason := latestReleaseUpdateSource(ctx, client, platform)
		if reason != "" {
			result := noUpdate(reason)
			result.Source = updateReleaseListURL
			return result
		}
		address, artifact = source.manifestURL, source.artifactURL
	}
	result := checkUpdateTrustedAt(ctx, client, address, artifact, platform, hash, receipt, updatetrust.PublisherKeys(), time.Now())
	if result.Status == "available" && !NativePackageUpdateSupported() {
		result.InstallerReady = false
		result.Reason = "当前发行版不支持内置安装或缺少安装工具，请从 GitHub Releases 下载对应安装包升级，保留原数据目录。"
	}
	return result
}

// A slow filesystem must not hold startup or the check API past the deadline.
// At most one detached probe exists because beginUpdateCheck runs once.
func boundedUpdateProbe(ctx context.Context, probe func() UpdateStatus) UpdateStatus {
	done := make(chan UpdateStatus, 1)
	go func() { done <- probe() }()
	select {
	case result := <-done:
		if ctx.Err() == nil {
			return result
		}
	case <-ctx.Done():
	}
	return noUpdate("timeout")
}
func checkUpdate(ctx context.Context, client *http.Client, address, platform, currentHash string, receipt UpdateReceipt) UpdateStatus {
	return checkUpdateTrusted(ctx, client, address, platform, currentHash, receipt, updatetrust.PublisherKeys(), time.Now())
}

// The application always supplies its compiled publisher keys. Tests use an
// independent key through this helper, never a production signing private key.
func checkUpdateTrusted(ctx context.Context, client *http.Client, address, platform, currentHash string, receipt UpdateReceipt, trusted map[string]ed25519.PublicKey, now time.Time) UpdateStatus {
	_, artifact := updateAddress(platform)
	return checkUpdateTrustedAt(ctx, client, address, artifact, platform, currentHash, receipt, trusted, now)
}

func checkUpdateTrustedAt(ctx context.Context, client *http.Client, address, artifact, platform, currentHash string, receipt UpdateReceipt, trusted map[string]ed25519.PublicKey, now time.Time) UpdateStatus {
	result := noUpdate("unavailable")
	result.Source = address
	result.Platform = platform
	if artifact == "" {
		result.Reason = "platform-unavailable"
		return result
	}
	req, e := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if e != nil {
		return result
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "DevShell/"+strings.TrimPrefix(ApplicationVersion, "v"))
	req.Header.Set("Cache-Control", "no-cache")
	response, e := client.Do(req)
	if e != nil {
		if ctx.Err() != nil {
			result.Reason = "timeout"
		}
		return result
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		// Release policy 2026-10-20: Linux payloads (up.deb / up.pkg.tar.zst)
		// are no longer published. A 404 on a Linux manifest means "retired",
		// not "network failure", so deployed Linux clients get an honest hint.
		if response.StatusCode == http.StatusNotFound && strings.HasPrefix(platform, "linux-") {
			result.Reason = "linux-discontinued"
		}
		return result
	}
	data, e := io.ReadAll(io.LimitReader(response.Body, 65537))
	if e != nil || len(data) > 65536 {
		result.Reason = "invalid"
		return result
	}
	var descriptor UpdateDescriptor
	if json.Unmarshal(data, &descriptor) != nil || descriptor.SchemaVersion != 2 || descriptor.Build == 0 || descriptor.Product != "DevShell" || descriptor.Platform != platform || len(descriptor.Notes) > 16000 || len(descriptor.Version) > 64 || strings.TrimSpace(descriptor.Version) == "" || len(descriptor.DisplayVersion) > 128 || !updateHashPattern.MatchString(descriptor.SHA256) || !updateHashPattern.MatchString(descriptor.ExecutableSHA256) || descriptor.Size <= 0 || descriptor.Size > maximumUpdateSize {
		result.Reason = "invalid"
		return result
	}
	// This branch publishes updates as GitHub Release assets: the fixed HTTPS
	// release host plus the SHA-256 checks below are the trust anchor, so an
	// unsigned manifest is accepted. If any signing field is present the
	// manifest claims a signature and the full Ed25519 chain must validate
	// before normalization or displaying notes.
	if descriptor.Signature != "" || descriptor.SigningKeyID != "" || descriptor.IssuedAt != 0 || descriptor.ExpiresAt != 0 {
		if updatetrust.Verify(descriptor, trusted, now) != nil {
			result.Reason = "invalid-signature"
			return result
		}
	}
	descriptor.SHA256 = strings.ToLower(descriptor.SHA256)
	descriptor.ExecutableSHA256 = strings.ToLower(descriptor.ExecutableSHA256)
	if platform == "windows-amd64" && descriptor.SHA256 != descriptor.ExecutableSHA256 {
		result.Reason = "invalid"
		return result
	}
	minimumBuild := ApplicationBuild
	if receipt.Build > minimumBuild && receiptAppliesToCurrentBuild(receipt.Build) {
		minimumBuild = receipt.Build
	}
	versionOrder, validVersion := compareUpdateVersions(descriptor.Version, ApplicationVersion)
	if !validVersion || versionOrder < 0 {
		result.Reason = "version-rollback"
		return result
	}
	// A manually installed newer build supersedes the older receipt's display
	// version. Its build high-water mark above still prevents replay/rollback.
	if receipt.Version != "" && receiptAppliesToCurrentBuild(receipt.Build) && (receipt.Build == 0 || receipt.Build >= ApplicationBuild) {
		order, valid := compareUpdateVersions(descriptor.Version, receipt.Version)
		if !valid || order < 0 {
			result.Reason = "version-rollback"
			return result
		}
	}
	if descriptor.Build <= minimumBuild {
		result.Reason = "build-rollback"
		if descriptor.Build == minimumBuild && strings.EqualFold(currentHash, descriptor.ExecutableSHA256) {
			result.Reason = "current"
		}
		return result
	}
	result.Status = "available"
	result.Reason = ""
	result.InstallerReady = true
	result.LatestVersion = descriptor.DisplayVersion
	if strings.TrimSpace(result.LatestVersion) == "" {
		result.LatestVersion = descriptor.Version
	}
	result.LatestBuild = descriptor.Build
	result.Notes = descriptor.Notes
	result.Package = &UpdatePackage{Format: updatePackageFormat(platform), Build: descriptor.Build, Version: descriptor.Version, URL: artifact, SHA256: descriptor.SHA256, Size: descriptor.Size, ExecutableSHA256: descriptor.ExecutableSHA256, expiresAt: descriptor.ExpiresAt}
	return result
}

// Numeric dotted release versions are deliberately strict. Unknown prerelease
// syntax cannot silently downgrade a stable release.
func compareUpdateVersions(left, right string) (int, bool) {
	parse := func(value string) ([]uint64, bool) {
		parts := strings.Split(strings.TrimPrefix(value, "v"), ".")
		if len(parts) < 2 || len(parts) > 4 {
			return nil, false
		}
		out := make([]uint64, len(parts))
		for i, part := range parts {
			if part == "" || len(part) > 10 {
				return nil, false
			}
			for _, c := range part {
				if c < '0' || c > '9' {
					return nil, false
				}
			}
			n, err := strconv.ParseUint(part, 10, 32)
			if err != nil {
				return nil, false
			}
			out[i] = n
		}
		return out, true
	}
	a, ok := parse(left)
	if !ok {
		return 0, false
	}
	b, ok := parse(right)
	if !ok {
		return 0, false
	}
	for i := 0; i < max(len(a), len(b)); i++ {
		var x, y uint64
		if i < len(a) {
			x = a[i]
		}
		if i < len(b) {
			y = b[i]
		}
		if x < y {
			return -1, true
		}
		if x > y {
			return 1, true
		}
	}
	return 0, true
}

// Update helpers recheck the installed high-water mark immediately before an
// installation, so a previously staged plan cannot roll back a newer receipt.
func ValidateUpdateBuild(configDir string, build uint64, version string) error {
	minimum := ApplicationBuild
	receipt, err := ReadUpdateReceipt(configDir)
	if err != nil {
		return err
	}
	if receiptAppliesToCurrentBuild(receipt.Build) {
		minimum = max(minimum, receipt.Build)
	}
	if build <= minimum {
		return errors.New("更新构建必须高于当前及已安装构建")
	}
	if order, ok := compareUpdateVersions(version, ApplicationVersion); !ok || order < 0 {
		return errors.New("更新版本不能低于当前版本")
	}
	if receipt.Version != "" && receiptAppliesToCurrentBuild(receipt.Build) && (receipt.Build == 0 || receipt.Build >= ApplicationBuild) {
		if order, ok := compareUpdateVersions(version, receipt.Version); !ok || order < 0 {
			return errors.New("更新版本不能低于已安装版本")
		}
	}
	return nil
}

func ReadUpdateReceipt(configDir string) (UpdateReceipt, error) {
	var receipt UpdateReceipt
	file, err := os.Open(filepath.Join(configDir, "update-receipt.json"))
	if os.IsNotExist(err) {
		return receipt, nil
	}
	if err != nil {
		return receipt, fmt.Errorf("无法读取更新收据：%w", err)
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, 65537))
	if err != nil || len(data) > 65536 || json.Unmarshal(data, &receipt) != nil {
		return receipt, errors.New("更新收据损坏，无法确认已安装构建")
	}
	return receipt, nil
}

// Reuse the config writer's private temporary file, fsync and platform atomic
// replacement (MoveFileEx with WRITE_THROUGH on Windows).
func WriteUpdateReceipt(configDir string, data []byte) error {
	if len(data) > 65536 || !json.Valid(data) {
		return errors.New("更新收据内容无效")
	}
	return atomicConfigFile(filepath.Join(configDir, "update-receipt.json"), data)
}

func (a *App) registerUpdateHTTP(mux *http.ServeMux) {
	mux.HandleFunc("GET /api/updates/check", func(w http.ResponseWriter, r *http.Request) {
		a.beginUpdateCheck()
		select {
		case <-a.updateCheck.done:
			writeJSON(w, a.getUpdateResult())
		case <-r.Context().Done():
		}
	})
	mux.HandleFunc("POST /api/updates/check", func(w http.ResponseWriter, r *http.Request) {
		a.updateCheck.manualMu.Lock()
		defer a.updateCheck.manualMu.Unlock()
		ctx, cancel := context.WithTimeout(r.Context(), manualUpdateTimeout)
		defer cancel()
		result := a.updateProbe(ctx, manualUpdateTimeout)
		a.setUpdateResult(result)
		writeJSON(w, result)
	})
	mux.HandleFunc("POST /api/updates/download", func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			SHA256 string `json:"sha256"`
		}
		if !decode(w, r, &request) {
			return
		}
		job, e := a.StartUpdateDownload(request.SHA256)
		respond(w, job, e)
	})
	mux.HandleFunc("GET /api/updates/download/{id}", func(w http.ResponseWriter, r *http.Request) {
		job, e := a.UpdateDownload(r.PathValue("id"))
		respond(w, job, e)
	})
	mux.HandleFunc("DELETE /api/updates/download/{id}", func(w http.ResponseWriter, r *http.Request) {
		a.updateCheck.mu.Lock()
		defer a.updateCheck.mu.Unlock()
		job := a.updateCheck.job
		if job == nil || job.ID != r.PathValue("id") {
			respond(w, nil, errors.New("更新下载不存在"))
			return
		}
		if job.Status == "downloading" {
			job.cancel()
		}
		writeJSON(w, map[string]bool{"ok": true})
	})
}
func (a *App) StartUpdateDownload(hash string) (UpdateDownload, error) {
	a.beginUpdateCheck()
	select {
	case <-a.updateCheck.done:
	default:
		return UpdateDownload{}, errors.New("更新检查尚未完成")
	}
	offer := a.getUpdateResult()
	if offer.Status == "available" && !NativePackageUpdateSupported() {
		return UpdateDownload{}, errors.New("当前发行版不支持内置安装或缺少安装工具，请下载对应安装包升级")
	}
	if offer.Status != "available" || offer.Package == nil || offer.Package.SHA256 != hash {
		return UpdateDownload{}, errors.New("更新文件与本次检查结果不一致")
	}
	if runtime.GOOS == "linux" && offer.Package.Format != NativePackageFormat() {
		return UpdateDownload{}, errors.New("更新安装包格式与当前发行版不匹配")
	}
	if offer.Package.expiresAt != 0 && offer.Package.expiresAt <= time.Now().Unix() {
		return UpdateDownload{}, errors.New("更新签名已过期，请重新启动程序检查更新")
	}
	if err := ValidateUpdateBuild(a.store.dir, offer.Package.Build, offer.Package.Version); err != nil {
		return UpdateDownload{}, err
	}
	a.updateCheck.mu.Lock()
	defer a.updateCheck.mu.Unlock()
	if old := a.updateCheck.job; old != nil && (old.Status == "downloading" || old.Status == "ready") {
		return *old, nil
	}
	dir, e := os.MkdirTemp(a.store.dir, ".update-")
	if e != nil {
		return UpdateDownload{}, e
	}
	release, e := holdUpdateDirectory(dir, true)
	if e != nil {
		_ = os.RemoveAll(dir)
		return UpdateDownload{}, e
	}
	ctx, cancel := context.WithTimeout(a.ctx, 15*time.Minute)
	job := &UpdateDownload{ID: randomID(), Status: "downloading", Total: offer.Package.Size, File: filepath.Join(dir, filepath.Base(offer.Package.URL)), Package: *offer.Package, cancel: cancel}
	job.releaseLease = release
	a.updateCheck.job = job
	go a.downloadUpdate(ctx, job)
	return *job, nil
}
func (a *App) UpdateDownload(id string) (UpdateDownload, error) {
	a.updateCheck.mu.Lock()
	defer a.updateCheck.mu.Unlock()
	if a.updateCheck.job == nil || a.updateCheck.job.ID != id {
		return UpdateDownload{}, errors.New("更新下载不存在")
	}
	return *a.updateCheck.job, nil
}
func (a *App) PreparedUpdate(id string) (UpdateDownload, error) {
	job, e := a.UpdateDownload(id)
	if e != nil {
		return job, e
	}
	if job.Status != "ready" {
		return job, errors.New("更新尚未下载完成")
	}
	if job.Package.expiresAt != 0 && job.Package.expiresAt <= time.Now().Unix() {
		return job, errors.New("更新签名已过期，请重新启动程序检查更新")
	}
	hash, e := FileSHA256(job.File)
	if e != nil {
		return job, e
	}
	if hash != job.Package.SHA256 {
		return job, errors.New("更新文件校验失败")
	}
	return job, nil
}
func (a *App) downloadUpdate(ctx context.Context, job *UpdateDownload) {
	defer job.cancel()
	err := func() error {
		req, e := http.NewRequestWithContext(ctx, http.MethodGet, job.Package.URL, nil)
		if e != nil {
			return e
		}
		response, e := updateClient(15 * time.Minute).Do(req)
		if e != nil {
			return e
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			return fmt.Errorf("下载失败 (%d)", response.StatusCode)
		}
		if response.ContentLength >= 0 && response.ContentLength != job.Total {
			return errors.New("更新文件大小与描述不符")
		}
		f, e := os.OpenFile(job.File, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			return e
		}
		defer f.Close()
		h := sha256.New()
		reader := io.LimitReader(response.Body, job.Total+1)
		buf := make([]byte, 128<<10)
		var total int64
		for {
			n, re := reader.Read(buf)
			if n > 0 {
				total += int64(n)
				if total > job.Total {
					return errors.New("更新文件超过描述的大小")
				}
				if _, e = f.Write(buf[:n]); e != nil {
					return e
				}
				h.Write(buf[:n])
				a.updateCheck.mu.Lock()
				job.Received = total
				a.updateCheck.mu.Unlock()
			}
			if re == io.EOF {
				break
			}
			if re != nil {
				return re
			}
		}
		if total != job.Total || hex.EncodeToString(h.Sum(nil)) != job.Package.SHA256 {
			return errors.New("更新文件 SHA-256 校验失败")
		}
		return f.Sync()
	}()
	a.finishUpdateDownload(ctx, job, err)
}
func (a *App) finishUpdateDownload(ctx context.Context, job *UpdateDownload, err error) {
	a.updateCheck.mu.Lock()
	defer a.updateCheck.mu.Unlock()
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		job.Status = "failed"
		job.Error = err.Error()
		if errors.Is(ctx.Err(), context.Canceled) {
			job.Status = "cancelled"
			job.Error = "下载已取消"
		}
		if job.releaseLease != nil {
			job.releaseLease()
		}
		_ = os.RemoveAll(filepath.Dir(job.File))
		return
	}
	job.Status = "ready"
}
