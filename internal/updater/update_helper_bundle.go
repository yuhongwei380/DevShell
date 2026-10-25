package updater

import (
	"archive/zip"
	"devshell/internal/app"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// A macOS update payload is a ZIP of the whole application bundle, published by
// scripts/build/build-macos.py with ditto --keepParent, so every entry lives
// under DevShell.app/. Replacing the bundle is a directory swap, which needs no
// privileged installer as long as the user owns the installation location.
const (
	bundlePackageFormat   = "app-zip"
	bundleRootName        = "DevShell.app"
	bundleExecutableName  = "DevShell"
	bundleBackupSuffix    = ".update-old"
	bundleStagePrefix     = ".update-new-"
	maximumBundleEntries  = 30000
	maximumBundleFileSize = 512 << 20
	maximumBundleTotal    = 4 << 30
)

// bundleFromExecutable maps a running bundle executable back to its .app
// directory, rejecting anything that is not the DevShell bundle layout so an
// update never renames an unrelated directory.
func bundleFromExecutable(target string) (string, error) {
	clean := filepath.Clean(target)
	if filepath.Base(clean) != bundleExecutableName {
		return "", errors.New("当前程序不在标准 .app 包内，无法在线更新")
	}
	inner := filepath.Base(filepath.Dir(clean))
	outer := filepath.Base(filepath.Dir(filepath.Dir(clean)))
	if inner != "MacOS" || outer != "Contents" {
		return "", errors.New("当前程序不在标准 .app 包内，无法在线更新")
	}
	bundle := filepath.Dir(filepath.Dir(filepath.Dir(clean)))
	if !strings.HasSuffix(bundle, ".app") {
		return "", errors.New("当前程序不在标准 .app 包内，无法在线更新")
	}
	return bundle, nil
}

func bundleExecutableIn(bundle string) string {
	return filepath.Join(bundle, "Contents", "MacOS", bundleExecutableName)
}

// directoryWritable proves the installation location can be changed, which is
// how a read-only volume (a DMG still mounted) or a root-owned /Applications
// entry is detected before the running application quits.
func directoryWritable(dir string) error {
	probe := filepath.Join(dir, ".dengshell-write-test")
	file, err := os.OpenFile(probe, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return fmt.Errorf("无法写入安装位置：%w", err)
	}
	name := file.Name()
	_ = file.Close()
	_ = os.Remove(name)
	return nil
}

func looksLikeZipArchive(path string) bool {
	file, err := os.Open(path)
	if err != nil {
		return false
	}
	defer file.Close()
	magic := make([]byte, 4)
	if _, err = io.ReadFull(file, magic); err != nil {
		return false
	}
	return string(magic) == "PK\x03\x04" || string(magic) == "PK\x05\x06"
}

// zipEntryPath validates one archive entry and returns its path under dir. A
// bundle archive must be self-contained: no absolute path, no traversal, and
// everything inside DevShell.app/.
func zipEntryPath(dir string, entry *zip.File) (string, error) {
	// Directory entries end with a slash, which must not read as an empty path
	// component.
	name := strings.TrimSuffix(filepath.ToSlash(entry.Name), "/")
	if name == "" || strings.HasPrefix(name, "/") || filepath.IsAbs(entry.Name) {
		return "", errors.New("应用压缩包包含非法路径")
	}
	parts := strings.Split(name, "/")
	if parts[0] != bundleRootName {
		return "", errors.New("应用压缩包根目录不是 DevShell.app")
	}
	for _, part := range parts {
		if part == "" || part == ".." || part == "." {
			return "", errors.New("应用压缩包包含非法路径")
		}
	}
	path := filepath.Join(append([]string{dir}, parts...)...)
	if path != dir && !strings.HasPrefix(path, dir+string(filepath.Separator)) {
		return "", errors.New("应用压缩包包含非法路径")
	}
	return path, nil
}

func shouldSkipZipEntry(entry *zip.File) bool {
	name := filepath.ToSlash(entry.Name)
	if strings.HasPrefix(name, "__MACOSX/") {
		return true
	}
	switch filepath.Base(name) {
	case ".DS_Store", "":
		return true
	}
	return strings.HasPrefix(filepath.Base(name), "._")
}

// unpackAppBundle extracts the bundle into stageDir and returns the bundle
// directory inside it. Modes and symlinks are restored because the bundle holds
// an executable and code signature that must survive the round trip.
func unpackAppBundle(archive, stageDir string) (string, error) {
	reader, err := zip.OpenReader(archive)
	if err != nil {
		return "", errors.New("下载的文件不是有效的应用压缩包")
	}
	defer reader.Close()
	if len(reader.File) == 0 || len(reader.File) > maximumBundleEntries {
		return "", errors.New("应用压缩包条目数量异常")
	}
	var total int64
	for _, entry := range reader.File {
		if shouldSkipZipEntry(entry) {
			continue
		}
		path, err := zipEntryPath(stageDir, entry)
		if err != nil {
			return "", err
		}
		mode := entry.Mode()
		if mode.IsDir() {
			if err = os.MkdirAll(path, 0755); err != nil {
				return "", err
			}
			continue
		}
		if entry.UncompressedSize64 > maximumBundleFileSize {
			return "", errors.New("应用压缩包内单个文件过大")
		}
		total += int64(entry.UncompressedSize64)
		if total > maximumBundleTotal {
			return "", errors.New("应用压缩包解压后过大")
		}
		if err = os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			return "", err
		}
		if err = extractZipEntry(entry, path, mode); err != nil {
			return "", err
		}
	}
	bundle := filepath.Join(stageDir, bundleRootName)
	if extracted, err := os.Stat(bundle); err != nil || !extracted.IsDir() {
		return "", errors.New("应用压缩包内没有找到 DevShell.app")
	}
	return bundle, nil
}

func extractZipEntry(entry *zip.File, path string, mode os.FileMode) error {
	source, err := entry.Open()
	if err != nil {
		return err
	}
	defer source.Close()
	if mode&os.ModeSymlink != 0 {
		link, err := io.ReadAll(io.LimitReader(source, 4096))
		if err != nil {
			return err
		}
		target := strings.TrimSpace(string(link))
		if target == "" {
			return errors.New("应用压缩包内的符号链接无效")
		}
		return os.Symlink(target, path)
	}
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode.Perm())
	if err != nil {
		return err
	}
	// The signature covers file bytes, so refuse a truncated entry instead of
	// writing a partial bundle that would only fail at first launch.
	written, err := io.Copy(file, io.LimitReader(source, int64(entry.UncompressedSize64)+1))
	if err == nil && written != int64(entry.UncompressedSize64) {
		err = errors.New("应用压缩包内容不完整")
	}
	if err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err == nil {
		err = closeErr
	}
	if err != nil {
		_ = os.Remove(path)
	}
	return err
}

// verifyUnpackedBundle checks the two files that define a launchable bundle and
// that the shipped executable is exactly the one the published descriptor
// promised, before anything on disk is renamed.
func verifyUnpackedBundle(bundle, executableSHA256 string) error {
	if _, err := os.Stat(filepath.Join(bundle, "Contents", "Info.plist")); err != nil {
		return errors.New("应用包缺少 Info.plist")
	}
	executable := bundleExecutableIn(bundle)
	info, err := os.Stat(executable)
	if err != nil {
		return errors.New("应用包缺少可执行程序")
	}
	if !bundleExecutableLaunchable(info) {
		return errors.New("应用包缺少可执行程序")
	}
	hash, err := app.FileSHA256(executable)
	if err != nil {
		return err
	}
	if hash != executableSHA256 {
		return errors.New("应用包内的程序与更新描述不一致")
	}
	return nil
}

// bundleExecutableLaunchable enforces the mode bits a bundle payload needs.
// Only POSIX hosts store executable bits, so a Windows test host cannot observe
// the property; the SHA-256 comparison above is what binds the payload there.
func bundleExecutableLaunchable(info os.FileInfo) bool {
	if runtime.GOOS == "windows" {
		return true
	}
	return info.Mode().Perm()&0111 != 0
}

// swapAppBundle replaces the installed bundle with the unpacked one. The old
// bundle keeps a known suffix so a failure after the rename can be undone by
// rollbackAppBundle even if this process dies in between.
func swapAppBundle(installed, unpacked string) error {
	if err := directoryWritable(filepath.Dir(installed)); err != nil {
		return err
	}
	backup := installed + bundleBackupSuffix
	if err := os.RemoveAll(backup); err != nil {
		return fmt.Errorf("无法清理上次更新的备份：%w", err)
	}
	if err := os.Rename(installed, backup); err != nil {
		return fmt.Errorf("无法移出当前应用包：%w", err)
	}
	if err := os.Rename(unpacked, installed); err != nil {
		if restore := os.Rename(backup, installed); restore != nil {
			return fmt.Errorf("应用包替换失败，且原应用包恢复失败（原版本在 %s）：%w", backup, err)
		}
		return fmt.Errorf("应用包替换失败：%w", err)
	}
	_ = os.RemoveAll(backup)
	return nil
}

// rollbackAppBundle restores the backup left by swapAppBundle.
func rollbackAppBundle(installed string) error {
	backup := installed + bundleBackupSuffix
	if _, err := os.Stat(backup); err != nil {
		return errors.New("原应用包备份不可用")
	}
	if _, err := os.Stat(installed); err == nil {
		if err := os.RemoveAll(installed); err != nil {
			return fmt.Errorf("无法移除新应用包：%w", err)
		}
	}
	return os.Rename(backup, installed)
}
