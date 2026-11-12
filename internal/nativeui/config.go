package nativeui

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// PortableConfigDir keeps user data beside portable binaries while using
// Application Support for a macOS app bundle. Native UI is the only desktop
// frontend, so this helper owns the desktop configuration path.
func PortableConfigDir() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", err
	}
	real, err := filepath.EvalSymlinks(executable)
	if err != nil {
		return "", err
	}
	absolute, err := filepath.Abs(real)
	if err != nil {
		return "", err
	}
	if runtime.GOOS == "darwin" && macOSApplicationBundle(absolute) {
		root, err := os.UserConfigDir()
		if err != nil {
			return "", err
		}
		current := filepath.Join(root, "DevShell")
		legacy := filepath.Join(root, "DengShell")
		if _, err := os.Stat(current); os.IsNotExist(err) {
			if _, legacyErr := os.Stat(legacy); legacyErr == nil {
				_ = os.Rename(legacy, current)
			}
		}
		return current, nil
	}
	return filepath.Join(filepath.Dir(absolute), "data"), nil
}

func macOSApplicationBundle(executable string) bool {
	macos := filepath.Dir(filepath.Clean(executable))
	contents := filepath.Dir(macos)
	bundle := filepath.Dir(contents)
	return filepath.Base(macos) == "MacOS" && filepath.Base(contents) == "Contents" && strings.HasSuffix(filepath.Base(bundle), ".app")
}
