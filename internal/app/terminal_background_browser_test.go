package app

import "testing"

// Regression guard for built-in image backgrounds silently not applying: the
// solid-black change matched every "builtin:" id as imageless. The script
// selects builtin:mist, sweeps the strength slider across themes, and compares
// actual rendered pixels, so an unapplied background fails immediately.
func TestTerminalBackgroundBrowser(t *testing.T) {
	runIsolatedNavigationBrowser(t, "test-terminal-background-browser.mjs")
}
