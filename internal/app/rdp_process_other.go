//go:build !windows

package app

import (
	"errors"
	"os/exec"
)

func configureRDPProcess(cmd *exec.Cmd)           {}
func ownRDPProcess(cmd *exec.Cmd) (func(), error) { return func() {}, nil }

func validateRDPParent(parent uintptr) error {
	return errors.New("此 RDP 测试版目前仅支持 Windows x64")
}

func validateRDPParentProcess(parent uintptr, expected uint32) error {
	return validateRDPParent(parent)
}

func (a *App) RDPViewport(id string, x, y, width, height, viewportWidth float64, visible bool) error {
	return errors.New("此 RDP 测试版目前仅支持 Windows x64")
}
func (a *App) RDPResolution(id string, width, height int) error {
	return errors.New("此 RDP 测试版目前仅支持 Windows x64")
}
func (a *App) RDPSendSecureAttention(id string) error {
	return errors.New("此 RDP 测试版目前仅支持 Windows x64")
}

func rdpHostDisplayArguments(parent uintptr) string { return "" }
