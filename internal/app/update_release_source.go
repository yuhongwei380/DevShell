package app

import (
	"context"
	"encoding/json"
	"encoding/xml"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// var, not const: tests point the release feed and the download base at a
// local server, mirroring how the runtime wraps them with the configured
// accelerator origin.
// RC discovery reads the public releases.atom feed on github.com instead of
// the REST release list: the anonymous API quota (60 requests per hour per
// IP) gets exhausted easily and surfaced as failed "unavailable" checks,
// while the feed costs no quota and accelerator mirrors already wrap
// github.com URLs exactly like release downloads. The feed lists published
// releases newest-first and drops deleted rc publishes, offering the same
// candidates the API list did; asset presence is then learned from the
// manifest fetch itself instead of a list payload.
var updateReleaseFeedURL = "https://github.com/yuhongwei380/DevShell/releases.atom"

var updateReleaseDownloadBase = "https://github.com/yuhongwei380/DevShell/releases/download/"

const maximumReleaseFeedSize = 2 << 20

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

// Minimal projection of the GitHub releases.atom feed: entry order is the
// publish order (newest first) and each entry's alternate link path carries
// the exact release tag.
type releaseFeed struct {
	XMLName xml.Name         `xml:"feed"`
	Entries []releaseFeedEntry `xml:"entry"`
}

type releaseFeedEntry struct {
	Links []releaseFeedLink `xml:"link"`
}

type releaseFeedLink struct {
	Rel  string `xml:"rel,attr"`
	Href string `xml:"href,attr"`
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

// releaseFeedTags extracts the tags one releases.atom feed publishes, in
// feed order. Tags are validated later by parseUpdateReleaseTag; entries
// whose links do not expose a /releases/tag/ path are skipped.
func releaseFeedTags(data []byte) ([]string, error) {
	var feed releaseFeed
	if err := xml.Unmarshal(data, &feed); err != nil {
		return nil, err
	}
	tags := make([]string, 0, len(feed.Entries))
	for _, entry := range feed.Entries {
		href := ""
		for _, link := range entry.Links {
			if link.Rel == "alternate" {
				href = link.Href
				break
			}
		}
		if href == "" && len(entry.Links) > 0 {
			href = entry.Links[0].Href
		}
		if _, tag, found := strings.Cut(href, "/releases/tag/"); found && tag != "" && !strings.Contains(tag, "/") {
			tags = append(tags, tag)
		}
	}
	return tags, nil
}

// RC clients inspect published release tags instead of /releases/latest, which
// intentionally excludes prereleases. Stable clients keep using that endpoint
// (it also costs no API quota). The third result reports whether the feed was
// served through the configured accelerator origin; a failing accelerator
// falls back to the direct URL.
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
	for _, candidate := range updateFetchAttempts(mirror, updateReleaseFeedURL) {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, candidate, nil)
		if err != nil {
			continue
		}
		req.Header.Set("Accept", "application/atom+xml, text/xml;q=0.9")
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
		viaMirror = candidate != updateReleaseFeedURL
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
	data, err := io.ReadAll(io.LimitReader(response.Body, maximumReleaseFeedSize+1))
	if err != nil || len(data) > maximumReleaseFeedSize {
		return releaseUpdateSource{}, "invalid", viaMirror
	}
	tags, err := releaseFeedTags(data)
	if err != nil {
		return releaseUpdateSource{}, "invalid", viaMirror
	}

	// Collect every publishable candidate, then prefer the newest functional
	// version. Inside one version, tag-encoded builds order first (descending)
	// and version-only tags follow in feed order — GitHub lists releases
	// newest-created first, so the first unknown candidate per version is its
	// newest publish and needs just one manifest fetch to learn its build.
	// The feed exposes no asset inventory, so a publish whose manifest was
	// never uploaded fails the manifest resolution below (and the full
	// manifest validation in checkUpdateTrustedAt) instead of a list pre-check.
	type releaseCandidate struct {
		version string
		build   uint64 // 0: version-only tag, build lives only in the manifest
		source  releaseUpdateSource
	}
	var candidates []releaseCandidate
	for _, tag := range tags {
		version, build, _, ok := parseUpdateReleaseTag(tag)
		if !ok {
			continue
		}
		versionOrder, validVersion := compareUpdateVersions(version, ApplicationVersion)
		if !validVersion || versionOrder < 0 {
			continue
		}
		base := updateReleaseDownloadBase + url.PathEscape(tag) + "/"
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
