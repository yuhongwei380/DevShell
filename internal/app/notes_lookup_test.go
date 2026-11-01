package app

import (
	"strconv"
	"strings"
	"testing"

	"devshell/docs/release"
)

// The update log must ship with the notes of the exact current rc, newest
// suffix-less naming first, and the shipped embed must actually contain them.
func TestCurrentReleaseNotesVersionsAndEmbeddedLookup(t *testing.T) {
	rc := strings.TrimPrefix(strings.TrimPrefix(ApplicationDisplayVersion, "ver.Phoenix · "), ApplicationVersion+"-rc.")
	if _, err := strconv.ParseUint(rc, 10, 32); err != nil {
		t.Fatalf("ApplicationDisplayVersion %q must carry an rc ordinal", ApplicationDisplayVersion)
	}
	wantFirst := ApplicationVersion + "-rc." + rc
	wantSecond := wantFirst + "-build" + strconv.FormatUint(ApplicationBuild%100000000, 10)
	candidates := currentReleaseNotesVersions()
	if len(candidates) != 2 || candidates[0] != wantFirst || candidates[1] != wantSecond {
		t.Fatalf("currentReleaseNotesVersions() = %v; want [%s %s]", candidates, wantFirst, wantSecond)
	}
	// The current rc ships the suffix-less file and the baseline; the legacy
	// -build name legitimately stops existing from this release onward.
	for _, candidate := range []string{wantFirst, ApplicationVersion} {
		if _, err := release.Read(candidate); err != nil {
			t.Fatalf("embedded notes for %s missing: %v", candidate, err)
		}
	}
}
