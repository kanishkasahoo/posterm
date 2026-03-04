#!/usr/bin/env bash

set -euo pipefail

export PATH="/usr/bin:/bin:/usr/sbin:/sbin"

REPO="kanishkasahoo/posterm"
INSTALL_PATH="/usr/local/bin/posterm"

log() {
  printf '%s\n' "$*"
}

err() {
  printf 'Error: %s\n' "$*" >&2
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "Missing required tool: $1"
    exit 1
  fi
}

normalize_sha256() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

read_expected_checksum() {
  local checksum_file="$1"
  local line token

  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      ""|\#*)
        continue
        ;;
    esac

    line="${line#"${line%%[![:space:]]*}"}"
    token="$line"
    case "$line" in
      *[[:space:]]*)
        token="${line%%[[:space:]]*}"
        ;;
    esac

    if [[ "$token" =~ ^[A-Fa-f0-9]{64}$ ]]; then
      normalize_sha256 "$token"
      return 0
    fi
  done <"$checksum_file"

  return 1
}

verify_checksum() {
  local archive_file="$1"
  local checksum_file="$2"
  local expected actual output

  if ! expected="$(read_expected_checksum "$checksum_file")"; then
    err "Invalid checksum file format: $checksum_file"
    exit 1
  fi

  case "$PLATFORM" in
    linux)
      if ! command -v sha256sum >/dev/null 2>&1; then
        err "Missing required tool: sha256sum"
        exit 1
      fi
      output="$(sha256sum "$archive_file")"
      ;;
    macos)
      if ! command -v shasum >/dev/null 2>&1; then
        err "Missing required tool: shasum"
        exit 1
      fi
      output="$(shasum -a 256 "$archive_file")"
      ;;
    *)
      err "Cannot verify checksum on unsupported platform: $PLATFORM"
      exit 1
      ;;
  esac

  actual="${output%% *}"
  actual="$(normalize_sha256 "$actual")"

  if [ "$actual" != "$expected" ]; then
    err "Checksum verification failed for $archive_file"
    exit 1
  fi
}

read_os_release_value() {
  local key="$1"
  local line value

  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      "$key="*)
        value="${line#*=}"
        case "$value" in
          \"*\")
            value="${value#\"}"
            value="${value%\"}"
            ;;
          \'*\')
            value="${value#\'}"
            value="${value%\'}"
            ;;
        esac
        printf '%s\n' "$value"
        return 0
        ;;
    esac
  done </etc/os-release

  return 1
}

select_archive_binary_member() {
  local archive_file="$1"
  local member base selected=""
  local found=0

  while IFS= read -r member || [ -n "$member" ]; do
    [ -z "$member" ] && continue

    case "$member" in
      /*|../*|*/../*|*/..)
        err "Archive contains unsafe path: $member"
        exit 1
        ;;
    esac

    case "$member" in
      */)
        continue
        ;;
    esac

    base="${member##*/}"
    if [ "$base" = "posterm" ]; then
      selected="$member"
      found=$((found + 1))
    fi
  done < <(tar -tzf "$archive_file")

  if [ "$found" -ne 1 ]; then
    err "Archive must contain exactly one posterm binary entry; found $found"
    exit 1
  fi

  printf '%s\n' "$selected"
}

detect_arch() {
  local machine
  machine="$(uname -m)"
  case "$machine" in
    x86_64|amd64)
      ARCH="amd64"
      ;;
    arm64|aarch64)
      ARCH="arm64"
      ;;
    *)
      err "Unsupported architecture: $machine. Supported architectures are x86_64/amd64 and arm64/aarch64."
      exit 1
      ;;
  esac
}

detect_platform() {
  local kernel
  kernel="$(uname -s)"

  case "$kernel" in
    Darwin)
      PLATFORM="macos"
      ASSET_NAME="posterm-macos.tar.gz"
      ;;
    Linux)
      if [ ! -f /etc/os-release ]; then
        err "Cannot determine Linux distribution: /etc/os-release not found. This installer only supports Ubuntu Linux."
        exit 1
      fi

      local os_id pretty_name
      os_id="$(read_os_release_value ID || true)"
      pretty_name="$(read_os_release_value PRETTY_NAME || true)"

      if [ "$os_id" != "ubuntu" ]; then
        if [ -z "$pretty_name" ]; then
          pretty_name="unknown"
        fi
        err "Unsupported Linux distribution: $pretty_name. This installer only supports Ubuntu Linux."
        exit 1
      fi

      PLATFORM="linux"
      ASSET_NAME="posterm-linux.tar.gz"
      ;;
    *)
      err "Unsupported operating system: $kernel. Supported operating systems are macOS and Ubuntu Linux."
      exit 1
      ;;
  esac
}

download_file() {
  local url="$1"
  local out="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fL --retry 3 --retry-delay 1 -o "$out" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$out" "$url"
  else
    err "Neither curl nor wget is installed. Please install one of them and retry."
    exit 1
  fi
}

install_binary() {
  local src="$1"
  local dst_dir

  dst_dir="${INSTALL_PATH%/*}"

  if [ -w "$dst_dir" ]; then
    mkdir -p "$dst_dir"
    install -m 0755 "$src" "$INSTALL_PATH"
  elif command -v sudo >/dev/null 2>&1; then
    sudo mkdir -p "$dst_dir"
    sudo install -m 0755 "$src" "$INSTALL_PATH"
  else
    err "No write permission to $dst_dir and sudo is not available. Run as a user with permission."
    exit 1
  fi
}

main() {
  if [ "$#" -gt 1 ]; then
    err "Usage: $0 [version]"
    err "Example: $0 v1.2.3"
    exit 1
  fi

  local version
  if [ "$#" -eq 1 ]; then
    version="$1"
  else
    version="latest"
  fi

  require_cmd uname
  require_cmd tar
  require_cmd mktemp
  require_cmd install

  detect_arch
  detect_platform

  local url checksum_url checksum_name
  if [ "$version" = "latest" ]; then
    url="https://github.com/$REPO/releases/latest/download/$ASSET_NAME"
    checksum_name="$ASSET_NAME.sha256"
    checksum_url="https://github.com/$REPO/releases/latest/download/$checksum_name"
  else
    url="https://github.com/$REPO/releases/download/$version/$ASSET_NAME"
    checksum_name="$ASSET_NAME.sha256"
    checksum_url="https://github.com/$REPO/releases/download/$version/$checksum_name"
  fi

  local tmpdir archive_path checksum_path extracted_bin tar_member
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  archive_path="$tmpdir/$ASSET_NAME"
  checksum_path="$tmpdir/$checksum_name"

  log "Detected OS: $PLATFORM"
  log "Detected architecture: $ARCH"
  log "Downloading $REPO ($version) from $url"

  if ! download_file "$url" "$archive_path"; then
    err "Failed to download release artifact from $url"
    exit 1
  fi

  if ! download_file "$checksum_url" "$checksum_path"; then
    err "Failed to download checksum file from $checksum_url"
    exit 1
  fi

  verify_checksum "$archive_path" "$checksum_path"

  if ! tar_member="$(select_archive_binary_member "$archive_path")"; then
    err "Failed to inspect archive: $archive_path"
    exit 1
  fi

  if ! tar -xzf "$archive_path" -C "$tmpdir" -- "$tar_member"; then
    err "Failed to extract binary from archive: $archive_path"
    exit 1
  fi

  extracted_bin="$tmpdir/$tar_member"
  if [ -L "$extracted_bin" ] || [ ! -f "$extracted_bin" ]; then
    err "Extracted posterm binary is not a regular file"
    exit 1
  fi

  chmod +x "$extracted_bin"
  install_binary "$extracted_bin"

  log "Installed posterm to $INSTALL_PATH"
  log "Run 'posterm --help' to verify installation."
}

main "$@"
