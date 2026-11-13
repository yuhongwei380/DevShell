package nativeui

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

// Run starts the native GPUI frontend against the already running local API.
// The API remains the single source of truth for configuration and sessions;
// the native process owns only window rendering and input dispatch.
func Run(baseURL, token string) error {
	return RunWithProcess(baseURL, token, nil)
}

// RunWithProcess registers the exact child before native window APIs may use it.
func RunWithProcess(baseURL, token string, started func(int)) error {
	path, err := executablePath()
	if err != nil {
		return err
	}
	command := exec.Command(path, "--base", baseURL, "--token", token)
	command.Stdout = os.Stdout
	command.Stderr = os.Stderr
	command.Dir = filepath.Dir(path)
	if err := command.Start(); err != nil {
		return err
	}
	if started != nil {
		started(command.Process.Pid)
	}
	return command.Wait()
}

// Available reports whether the native frontend can be launched from the
// current installation. An explicit DEVSHELL_NATIVE_UI path takes priority.
func Available() bool {
	_, err := executablePath()
	return err == nil
}

func executablePath() (string, error) {
	if configured := os.Getenv("DEVSHELL_NATIVE_UI"); configured != "" {
		if _, err := os.Stat(configured); err != nil {
			return "", fmt.Errorf("原生前端路径无效：%w", err)
		}
		return configured, nil
	}
	current, err := os.Executable()
	if err != nil {
		return "", err
	}
	directory := filepath.Dir(current)
	names := []string{"DevShellUI", "devshell-native-ui"}
	if runtime.GOOS == "windows" {
		for i := range names {
			names[i] += ".exe"
		}
	}
	for _, name := range names {
		candidate := filepath.Join(directory, name)
		if _, err := os.Stat(candidate); err == nil {
			return candidate, nil
		}
	}
	return "", errors.New("未找到 DevShell 原生前端，请先构建 native-ui 或设置 DEVSHELL_NATIVE_UI")
}
