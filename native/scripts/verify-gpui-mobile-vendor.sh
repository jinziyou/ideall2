#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native_dir="$(cd "${script_dir}/.." && pwd)"
workspace_dir="$(cd "${native_dir}/.." && pwd)"
vendor_dir="${native_dir}/vendor/gpui-mobile"
source_file="${vendor_dir}/upstream.env"
checksum_file="${vendor_dir}/PATCHED_FILES.sha256"
mode="${1:-}"

if [[ -n "${mode}" && "${mode}" != "--upstream" ]]; then
  echo "usage: $0 [--upstream]" >&2
  exit 2
fi

# shellcheck disable=SC1090
source "${source_file}"
: "${GPUI_MOBILE_UPSTREAM_URL:?missing upstream URL}"
: "${GPUI_MOBILE_UPSTREAM_REVISION:?missing upstream revision}"
: "${GPUI_MOBILE_GPUI_REVISION:?missing GPUI revision}"

[[ "${GPUI_MOBILE_UPSTREAM_URL}" == "https://github.com/itsbalamurali/gpui-mobile.git" ]] || {
  echo "unexpected gpui-mobile upstream: ${GPUI_MOBILE_UPSTREAM_URL}" >&2
  exit 1
}
[[ "${GPUI_MOBILE_UPSTREAM_REVISION}" =~ ^[0-9a-f]{40}$ ]] || {
  echo "gpui-mobile upstream revision must be a full commit hash" >&2
  exit 1
}
[[ "${GPUI_MOBILE_GPUI_REVISION}" =~ ^[0-9a-f]{40}$ ]] || {
  echo "GPUI revision must be a full commit hash" >&2
  exit 1
}
grep -F "${GPUI_MOBILE_UPSTREAM_REVISION}" "${vendor_dir}/README.md" >/dev/null
grep -F "${GPUI_MOBILE_GPUI_REVISION}" "${vendor_dir}/README.md" >/dev/null
gpui_manifest_revisions="$(
  sed -n \
    's|^.*git = "https://github.com/zed-industries/zed", rev = "\([0-9a-f]*\)".*$|\1|p' \
    "${vendor_dir}/Cargo.toml" |
    LC_ALL=C sort -u
)"
[[ "${gpui_manifest_revisions}" == "${GPUI_MOBILE_GPUI_REVISION}" ]] || {
  echo "gpui-mobile Cargo.toml drifted from the approved GPUI revision: ${gpui_manifest_revisions}" >&2
  exit 1
}
gpui_lock_sources="$(
  sed -n \
    's|^source = "git+https://github.com/zed-industries/zed?rev=[0-9a-f]*#\([0-9a-f]*\)"$|\1|p' \
    "${native_dir}/Cargo.lock" |
    LC_ALL=C sort -u
)"
[[ "${gpui_lock_sources}" == "${GPUI_MOBILE_GPUI_REVISION}" ]] || {
  echo "native Cargo.lock drifted from the approved GPUI revision: ${gpui_lock_sources}" >&2
  exit 1
}
if grep -F 'git+https://github.com/zed-industries/wgpu.git' "${native_dir}/Cargo.lock" >/dev/null; then
  echo "native Cargo.lock must not contain the retired Git wgpu source" >&2
  exit 1
fi
grep -A3 '^name = "wgpu"$' "${native_dir}/Cargo.lock" |
  grep -F 'version = "29.0.4"' >/dev/null || {
  echo "native Cargo.lock must contain the approved wgpu 29.0.4 release" >&2
  exit 1
}
if command -v sha256sum >/dev/null; then
  checksum_command=(sha256sum --check)
elif command -v shasum >/dev/null; then
  checksum_command=(shasum -a 256 --check)
else
  echo "sha256sum or shasum is required" >&2
  exit 1
fi
(cd "${workspace_dir}" && "${checksum_command[@]}" "${checksum_file}")

if [[ "${mode}" != "--upstream" ]]; then
  echo "gpui-mobile vendor integrity verified offline"
  exit 0
fi

command -v git >/dev/null || {
  echo "git is required for upstream provenance verification" >&2
  exit 1
}

checkout_dir="$(mktemp -d "${TMPDIR:-/tmp}/ideall-gpui-mobile.XXXXXX")"
cleanup() {
  rm -rf -- "${checkout_dir:?}"
}
trap cleanup EXIT

git -C "${checkout_dir}" init --quiet
git -C "${checkout_dir}" fetch \
  --quiet \
  --depth=1 \
  "${GPUI_MOBILE_UPSTREAM_URL}" \
  "${GPUI_MOBILE_UPSTREAM_REVISION}"
git -C "${checkout_dir}" checkout --quiet --detach FETCH_HEAD

actual_revision="$(git -C "${checkout_dir}" rev-parse HEAD)"
[[ "${actual_revision}" == "${GPUI_MOBILE_UPSTREAM_REVISION}" ]] || {
  echo "fetched unexpected gpui-mobile revision: ${actual_revision}" >&2
  exit 1
}

for license in LICENSE-AGPL LICENSE-APACHE LICENSE-GPL; do
  cmp "${checkout_dir}/${license}" "${vendor_dir}/${license}"
done

diff \
  <(cd "${checkout_dir}/src" && find . -type f -print | LC_ALL=C sort) \
  <(cd "${vendor_dir}/src" && find . -type f -print | LC_ALL=C sort)

changed_files=()
while IFS= read -r relative_path; do
  if ! cmp -s "${checkout_dir}/src/${relative_path}" "${vendor_dir}/src/${relative_path}"; then
    changed_files+=("${relative_path}")
  fi
done < <(
  cd "${checkout_dir}/src"
  find . -type f -print | sed 's|^\./||' | LC_ALL=C sort
)

expected_changed_files=(
  "android/dispatcher.rs"
  "android/display.rs"
  "android/jni.rs"
  "android/platform.rs"
  "components/material/progress_indicator.rs"
  "ios/dispatcher.rs"
  "ios/display.rs"
  "ios/ffi.rs"
  "ios/platform.rs"
  "ios/window.rs"
  "lib.rs"
  "momentum.rs"
  "target_platform.rs"
)
[[ "${changed_files[*]}" == "${expected_changed_files[*]}" ]] || {
  printf 'unexpected gpui-mobile source differences:\n' >&2
  printf '  %s\n' "${changed_files[@]}" >&2
  exit 1
}

echo "gpui-mobile vendor provenance verified at ${GPUI_MOBILE_UPSTREAM_REVISION}"
