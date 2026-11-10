package app

import (
	"bytes"
	"context"
	"encoding/binary"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
	"unicode/utf16"
)

const wslProbeTTL = 5 * time.Minute

// wslProbe caches the `wsl -l -q` result so window bootstraps (which answer
// localShellSupported for the frontend) never spawn a probe per page load.
var wslProbe struct {
	mu        sync.Mutex
	checking  time.Time
	available bool
}

// wslExecutable resolves the built-in WSL launcher. It ships with the WSL
// feature itself; on machines without WSL the file simply is not there.
func wslExecutable() string {
	root := os.Getenv("SystemRoot")
	if root == "" {
		root = `C:\Windows`
	}
	return filepath.Join(root, "System32", "wsl.exe")
}

// wslAvailable reports whether WSL has at least one installed distro. The
// result is cached for wslProbeTTL; installs picked up while running only
// matter after the cache ages out or the app restarts.
func wslAvailable() bool {
	if runtime.GOOS != "windows" {
		return false
	}
	wslProbe.mu.Lock()
	defer wslProbe.mu.Unlock()
	if !wslProbe.checking.IsZero() && time.Since(wslProbe.checking) < wslProbeTTL {
		return wslProbe.available
	}
	wslProbe.checking = time.Now()
	wslProbe.available = probeWsl()
	return wslProbe.available
}

// localShellSupported is the single platform gate shared by the bootstrap
// payload, the settings patch and the spawn path: macOS (and CI Linux) always;
// Windows only when WSL has an installed distro. Without WSL Windows keeps the
// historical default: feature hidden, “+” opens the start tab.
func localShellSupported() bool {
	return runtime.GOOS != "windows" || wslAvailable()
}

func probeWsl() bool {
	executable := wslExecutable()
	if info, err := os.Stat(executable); err != nil || info.IsDir() {
		return false
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	output, err := exec.CommandContext(ctx, executable, "-l", "-q").Output()
	if err != nil {
		return false
	}
	return len(wslDistroNames(output)) > 0
}

// wslDistroNames parses `wsl -l -q`. wsl.exe writes UTF-16LE (with or without
// a BOM) on every modern build; plain UTF-8 is accepted as a fallback. Blank
// lines are ignored, so a machine with the launcher but no distro yields none.
func wslDistroNames(raw []byte) []string {
	if bytes.ContainsRune(raw, 0) {
		if bytes.HasPrefix(raw, []byte{0xFF, 0xFE}) {
			raw = raw[2:]
		}
		raw = raw[:len(raw)-len(raw)%2]
		units := make([]uint16, 0, len(raw)/2)
		for i := 0; i < len(raw); i += 2 {
			units = append(units, binary.LittleEndian.Uint16(raw[i:i+2]))
		}
		raw = []byte(string(utf16.Decode(units)))
	}
	names := []string{}
	for _, line := range strings.Split(strings.ReplaceAll(string(raw), "\r\n", "\n"), "\n") {
		if name := strings.TrimSpace(line); name != "" {
			names = append(names, name)
		}
	}
	return names
}
