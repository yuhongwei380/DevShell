package app

import (
	"strconv"
	"strings"
	"testing"

	"devshell/docs/release"
)

// The update log must ship with the notes of the exact current version,
// newest suffix-less naming first, and the shipped embed must actually
// contain them. Release builds display the plain vX.Y.Z and ship only the
// aggregated release notes; rc builds additionally ship the legacy
// -build name for the historical tag format.
func TestCurrentReleaseNotesVersionsAndEmbeddedLookup(t *testing.T) {
	display := strings.TrimSpace(strings.TrimPrefix(ApplicationDisplayVersion, "ver.Phoenix · "))
	candidates := currentReleaseNotesVersions()
	if display == ApplicationVersion {
		if len(candidates) != 1 || candidates[0] != ApplicationVersion {
			t.Fatalf("currentReleaseNotesVersions() = %v; want [%s]", candidates, ApplicationVersion)
		}
	} else {
		rc := strings.TrimPrefix(display, ApplicationVersion+"-rc.")
		if _, err := strconv.ParseUint(rc, 10, 32); err != nil {
			t.Fatalf("ApplicationDisplayVersion %q must equal ApplicationVersion or carry an rc ordinal", ApplicationDisplayVersion)
		}
		wantFirst := ApplicationVersion + "-rc." + rc
		wantSecond := wantFirst + "-build" + strconv.FormatUint(ApplicationBuild%100000000, 10)
		if len(candidates) != 2 || candidates[0] != wantFirst || candidates[1] != wantSecond {
			t.Fatalf("currentReleaseNotesVersions() = %v; want [%s %s]", candidates, wantFirst, wantSecond)
		}
	}
	for _, candidate := range append(append([]string{}, candidates...), ApplicationVersion) {
		if _, err := release.Read(candidate); err != nil {
			t.Fatalf("embedded notes for %s missing: %v", candidate, err)
		}
	}
}
