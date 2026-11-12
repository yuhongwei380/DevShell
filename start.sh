#!/usr/bin/env bash
set -euo pipefail
cloudshell_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$cloudshell_dir"

dengshell_problem() {
  local message="$1"
  printf '%s\n' "$message" >&2
}

# Source checkouts build the Go host and GPUI frontend on demand. Release
# packages already contain both binaries beside this launcher.
if [[ ! -f ./dengshell || ! -x ./DevShellUI ]]; then
  if [[ -x ./build.sh ]]; then
    ./build.sh
  else
    dengshell_problem $'找不到 DevShell 程序文件。请重新解压完整发行包，或在源码目录运行 build.sh。'
    exit 1
  fi
fi

if [[ ! -x ./dengshell ]]; then
  if ! chmod u+x ./dengshell; then
    dengshell_problem $'无法设置 DevShell 的执行权限，请把程序复制到可写目录后重试。'
    exit 1
  fi
fi

exec ./dengshell "$@"
