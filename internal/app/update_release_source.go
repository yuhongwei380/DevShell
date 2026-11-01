package app

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// var, not const: tests point the release list and the download base at a
// local server, mirroring how the runtime wraps them with the configured
// accelerator origin.
var updateReleaseListURL = "https://api.github.com/repos/yuhongwei380/DevShell/releases?per_page=100"

var updateReleaseDownloadBase = "https://github.com/yuhongwei380/DevShell/releases/download/"

const maximumReleaseListSize = 2 << 20

var releaseCandidateDisplayPattern = regexp.MustCompile(`(?i)-rc\.?[0-9]+(?:\b|$)`)

// New-style tags carry the version only (vX.Y.Z / vX.Y.Z-rc.N, matching the
// release title rule); the anti-rollback build then lives only in the release
// manifest. Historical tags keep the -build<8-digit abbreviated or 10/11-digit
// full build> suffix and decode directly.
var updateReleaseTagPattern = regexp.MustCompile(`^(v[0-9]+\.[0-9]+\.[0-9]+)(-rc\.?[0-9]+)?(?:-build([0-9]{8}|[0-9]{10,11}))?$`)

type releaseUpdateSource struct {
	manifestURL string
	artifactURL string
	version     string
	build       uint64
}

type githubRelease struct {
	TagName    string               `json:"tag_name"`
	Draft      bool                 `json:"draft"`
	Prerelease bool                 `json:"prerelease"`
	Assets     []githubReleaseAsset `json:"assets"`
}

type githubReleaseAsset struct {
	Name  string `json:"name"`
	State string `json:"state"`
}

func isReleaseCandidateBuild() bool {
	return releaseCandidateDisplayPattern.MatchString(ApplicationDisplayVersion)
}

// parseUpdateReleaseTag decodes a release tag into its functional version and
// anti-rollback build. New-style tags (version only) return build 0, meaning
// the build is not encoded in the tag and lives only in the release manifest;
// callers must resolve it there before ordering. Historical tags decode fully.
func parseUpdateReleaseTag(tag string) (string, uint64, bool, bool) {
	parts := updateReleaseTagPattern.FindStringSubmatch(tag)
	if len(parts) != 4 {
		return "", 0, false, false
	}
	if parts[3] == "" {
		return parts[1], 0, parts[2] != "", true
	}
	buildText := parts[3]
	if len(buildText) == 8 {
		buildText = strconv.FormatUint(ApplicationBuild/100_000_000, 10) + buildText
	}
	build, err := strconv.ParseUint(buildText, 10, 64)
	if err != nil || build == 0 {
		return "", 0, false, false
	}
	return parts[1], build, parts[2] != "", true
}

// RC clients inspect published release tags instead of /releases/latest, which
// intentionally excludes prereleases. Stable clients keep using that endpoint.
// The third result reports whether the list was served through the configured
// accelerator origin; a failing accelerator falls back to the direct API.
func latestReleaseUpdateSource(ctx context.Context, client *http.Client, platform, mirror string) (releaseUpdateSource, string, bool) {
	manifestName, artifactName := updateAssetNames(platform)
	if manifestName == "" {
		return releaseUpdateSource{}, "platform-unavailable", false
	}
	if strings.HasPrefix(platform, "linux-") {
		return releaseUpdateSource{}, "linux-discontinued", false
	}
	var response *http.Response
	viaMirror, notFound := false, false
	for _, candidate := range updateFetchAttempts(mirror, updateReleaseListURL) {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, candidate, nil)
		if err != nil {
			continue
		}
		req.Header.Set("Accept", "application/vnd.github+json")
		req.Header.Set("X-GitHub-Api-Version", "2026-03-10")
		req.Header.Set("User-Agent", "DevShell/"+strings.TrimPrefix(ApplicationVersion, "v"))
		req.Header.Set("Cache-Control", "no-cache")
		candidateResponse, err := client.Do(req)
		if err != nil {
			continue
		}
		if candidateResponse.StatusCode == http.StatusNotFound {
			notFound = true
			candidateResponse.Body.Close()
			continue
		}
		if candidateResponse.StatusCode != http.StatusOK {
			candidateResponse.Body.Close()
			continue
		}
		response = candidateResponse
		viaMirror = candidate != updateReleaseListURL
		break
	}
	if response == nil {
		if ctx.Err() != nil {
			return releaseUpdateSource{}, "timeout", false
		}
		if notFound {
			return releaseUpdateSource{}, "release-not-found", false
		}
		return releaseUpdateSource{}, "unavailable", false
	}
	defer response.Body.Close()
	data, err := io.ReadAll(io.LimitReader(response.Body, maximumReleaseListSize+1))
	if err != nil || len(data) > maximumReleaseListSize {
		return releaseUpdateSource{}, "invalid", viaMirror
	}
	var releases []githubRelease
	if json.Unmarshal(data, &releases) != nil {
		return releaseUpdateSource{}, "invalid", viaMirror
	}

	// Collect every publishable candidate, then prefer the newest functional
	// version. Inside one version, tag-encoded builds order first (descending)
	// and version-only tags follow in API list order — GitHub returns releases
	// newest-created first, so the first unknown candidate per version is its
	// newest publish and needs just one manifest fetch to learn its build.
	type releaseCandidate struct {
		version string
		build   uint64 // 0: version-only tag, build lives only in the manifest
		source  releaseUpdateSource
	}
	var candidates []releaseCandidate
	for _, release := range releases {
		if release.Draft {
			continue
		}
		version, build, prereleaseTag, ok := parseUpdateReleaseTag(release.TagName)
		if !ok || prereleaseTag != release.Prerelease {
			continue
		}
		versionOrder, validVersion := compareUpdateVersions(version, ApplicationVersion)
		if !validVersion || versionOrder < 0 || !releaseHasUpdateAssets(release.Assets, manifestName, artifactName) {
			continue
		}
		base := updateReleaseDownloadBase + url.PathEscape(release.TagName) + "/"
		candidates = append(candidates, releaseCandidate{
			version: version,
			build:   build,
			source: releaseUpdateSource{
				manifestURL: base + url.PathEscape(manifestName),
				artifactURL: base + url.PathEscape(artifactName),
				version:     version,
			},
		})
	}
	sort.SliceStable(candidates, func(i, j int) bool {
		if order, _ := compareUpdateVersions(candidates[i].version, candidates[j].version); order != 0 {
			return order > 0
		}
		return candidates[i].build > candidates[j].build
	})
	var best releaseUpdateSource
	found := false
	resolvedUnknown := map[string]bool{}
	for _, candidate := range candidates {
		if candidate.build == 0 {
			if resolvedUnknown[candidate.version] || ctx.Err() != nil {
				continue
			}
			resolvedUnknown[candidate.version] = true
			build, ok := resolveReleaseBuild(ctx, client, mirror, candidate.source.manifestURL)
			if !ok {
				continue
			}
			candidate.build = build
		}
		if found {
			order, _ := compareUpdateVersions(candidate.version, best.version)
			if order < 0 || (order == 0 && candidate.build <= best.build) {
				continue
			}
		}
		candidate.source.build = candidate.build
		best = candidate.source
		found = true
	}
	if !found {
		return releaseUpdateSource{}, "release-not-found", viaMirror
	}
	return best, "", viaMirror
}

// resolveReleaseBuild reads the anti-rollback build from one release manifest
// so version-only tags can still be ordered across publishes. This is only an
// ordering hint: the winning manifest is re-fetched and fully checked (trust
// anchor, product/platform match, high-water mark, SHA-256) by
// checkUpdateTrustedAt before anything is offered or downloaded.
func resolveReleaseBuild(ctx context.Context, client *http.Client, mirror, manifestURL string) (uint64, bool) {
	for _, candidate := range updateFetchAttempts(mirror, manifestURL) {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, candidate, nil)
		if err != nil {
			continue
		}
		req.Header.Set("Accept", "application/json")
		req.Header.Set("User-Agent", "DevShell/"+strings.TrimPrefix(ApplicationVersion, "v"))
		req.Header.Set("Cache-Control", "no-cache")
		response, err := client.Do(req)
		if err != nil {
			continue
		}
		data, err := io.ReadAll(io.LimitReader(response.Body, 65537))
		status := response.StatusCode
		response.Body.Close()
		if err != nil || len(data) > 65536 || status != http.StatusOK {
			continue
		}
		var descriptor UpdateDescriptor
		if json.Unmarshal(data, &descriptor) != nil || descriptor.Build == 0 {
			continue
		}
		return descriptor.Build, true
	}
	return 0, false
}

func releaseHasUpdateAssets(assets []githubReleaseAsset, manifest, artifact string) bool {
	hasManifest, hasArtifact := false, false
	for _, asset := range assets {
		if asset.State != "uploaded" {
			continue
		}
		if asset.Name == manifest {
			hasManifest = true
		}
		if asset.Name == artifact {
			hasArtifact = true
		}
	}
	return hasManifest && hasArtifact
}
