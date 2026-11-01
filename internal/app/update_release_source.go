package app

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
)

// var, not const: tests point the release list at a local server, mirroring
// how the runtime wraps it with the configured accelerator origin.
var updateReleaseListURL = "https://api.github.com/repos/yuhongwei380/DevShell/releases?per_page=100"

const updateReleaseDownloadBase = "https://github.com/yuhongwei380/DevShell/releases/download/"
const maximumReleaseListSize = 2 << 20

var releaseCandidateDisplayPattern = regexp.MustCompile(`(?i)-rc\.?[0-9]+(?:\b|$)`)
var updateReleaseTagPattern = regexp.MustCompile(`^(v[0-9]+\.[0-9]+\.[0-9]+)(-rc\.?[0-9]+)?-build([0-9]{8}|[0-9]{10,11})$`)

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

func parseUpdateReleaseTag(tag string) (string, uint64, bool, bool) {
	parts := updateReleaseTagPattern.FindStringSubmatch(tag)
	if len(parts) != 4 {
		return "", 0, false, false
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

	var best releaseUpdateSource
	var found bool
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
		if found {
			order, _ := compareUpdateVersions(version, best.version)
			if order < 0 || (order == 0 && build <= best.build) {
				continue
			}
		}
		base := updateReleaseDownloadBase + url.PathEscape(release.TagName) + "/"
		best = releaseUpdateSource{
			manifestURL: base + url.PathEscape(manifestName),
			artifactURL: base + url.PathEscape(artifactName),
			version:     version,
			build:       build,
		}
		found = true
	}
	if !found {
		return releaseUpdateSource{}, "release-not-found", viaMirror
	}
	return best, "", viaMirror
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
