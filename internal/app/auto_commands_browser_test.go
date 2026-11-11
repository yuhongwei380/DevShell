package app

import "testing"

// Exercises the issue #5 feature end to end in an isolated browser: the
// Settings → Other features master switch, the connection editor command
// block, store merge semantics through the real API, and the prompt-gated
// typing queue driven by shell integration signals. No remote servers.
func TestAutoCommandsBrowser(t *testing.T) {
	runIsolatedNavigationBrowser(t, "test-auto-commands-browser.mjs")
}
