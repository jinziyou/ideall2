#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native_dir="$(cd "${script_dir}/.." && pwd)"
project_dir="${native_dir}/apps/ideall-mobile/platforms/ios"
profile="${IDEALL_IOS_BUILD_PROFILE:-release}"
configuration="${IDEALL_IOS_CONFIGURATION:-Release}"
result_bundle="${IDEALL_IOS_RESULT_BUNDLE:-${TMPDIR:-/tmp}/ideall-ios-smoke.xcresult}"

command -v xcrun >/dev/null || {
  echo "Xcode command-line tools are required" >&2
  exit 1
}
[[ -d "${project_dir}/Ideall.xcodeproj" ]] || {
  echo "generate the iOS project with build-mobile.sh before running the smoke test" >&2
  exit 1
}
[[ ! -e "${result_bundle}" ]] || {
  echo "iOS result bundle path already exists: ${result_bundle}" >&2
  exit 1
}

runtime="$(
  xcrun simctl list runtimes available |
    sed -n 's/.* - \(com\.apple\.CoreSimulator\.SimRuntime\.iOS-[^ ]*\)$/\1/p' |
    tail -1
)"
[[ -n "${runtime}" ]] || {
  echo "no available iOS Simulator runtime" >&2
  exit 1
}

device_name="ideall-native-smoke-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}"
device="$(
  xcrun simctl create \
    "${device_name}" \
    com.apple.CoreSimulator.SimDeviceType.iPhone-16 \
    "${runtime}"
)"
cleanup() {
  xcrun simctl shutdown "${device}" >/dev/null 2>&1 || true
  xcrun simctl delete "${device}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

xcrun simctl boot "${device}"
xcrun simctl bootstatus "${device}" -b

test_status=0
xcodebuild \
  -project "${project_dir}/Ideall.xcodeproj" \
  -scheme Ideall \
  -configuration "${configuration}" \
  -sdk iphonesimulator \
  -destination "platform=iOS Simulator,id=${device}" \
  -derivedDataPath "${project_dir}/build" \
  -resultBundlePath "${result_bundle}" \
  IDEALL_RUST_PROFILE="${profile}" \
  CODE_SIGNING_ALLOWED=NO \
  test ||
  test_status=$?

if [[ "${test_status}" -ne 0 ]]; then
  xcrun simctl spawn "${device}" log show \
    --last 10m \
    --style compact \
    --predicate \
    'process == "ideall" OR process == "ReportCrash" OR eventMessage CONTAINS[c] "com.jinziyou.ideall"' \
    || true
  find \
    "${HOME}/Library/Logs/DiagnosticReports" \
    "${HOME}/Library/Developer/CoreSimulator/Devices/${device}" \
    -type f \
    \( -name 'ideall*.crash' -o -name 'ideall*.ips' \) \
    -mmin -15 \
    -exec cat {} + \
    2>/dev/null \
    || true
  exit "${test_status}"
fi

echo "iOS interactive XCTest smoke passed; result bundle: ${result_bundle}"
