#!/usr/bin/env bash
set -euo pipefail

binary="${1:-}"
maximum_glibc="${2:-2.35}"

[[ -n "${binary}" && -f "${binary}" ]] || {
  echo "usage: $0 BINARY [MAXIMUM_GLIBC]" >&2
  exit 2
}
[[ "${maximum_glibc}" =~ ^[0-9]+\.[0-9]+$ ]] || {
  echo "MAXIMUM_GLIBC must look like 2.35" >&2
  exit 2
}
command -v readelf >/dev/null || {
  echo "readelf is required to verify the Linux release binary" >&2
  exit 1
}

file "${binary}" | grep -E 'ELF 64-bit.*(x86-64|ARM aarch64)' >/dev/null

newest_glibc="$({
  readelf --version-info "${binary}" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' || true
} | sed 's/^GLIBC_//' | sort -Vu | tail -1)"
[[ -n "${newest_glibc}" ]] || {
  echo "no GLIBC symbol versions found in ${binary}" >&2
  exit 1
}
highest_allowed="$(printf '%s\n%s\n' "${newest_glibc}" "${maximum_glibc}" | sort -V | tail -1)"
[[ "${highest_allowed}" == "${maximum_glibc}" ]] || {
  echo "${binary} requires GLIBC_${newest_glibc}; release maximum is GLIBC_${maximum_glibc}" >&2
  exit 1
}

forbidden="$({
  readelf -d "${binary}" | grep -oE 'Shared library: \[[^]]+\]' || true
} | grep -Ei 'libgtk|libwebkit|libjavascriptcore' || true)"
[[ -z "${forbidden}" ]] || {
  echo "Linux release unexpectedly links desktop WebView libraries:" >&2
  echo "${forbidden}" >&2
  exit 1
}

echo "verified Linux binary: GLIBC_${newest_glibc} <= GLIBC_${maximum_glibc}; no GTK/WebKit linkage"
