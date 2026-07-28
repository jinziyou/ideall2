#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_dir="$(cd "${script_dir}/../.." && pwd)"
platform="${1:-}"
profile="${2:-release}"
version="${IDEALL_VERSION:-0.2.0}"
numeric_version="${version%%[-+]*}"
version_code="${IDEALL_VERSION_CODE:-2}"

[[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || {
  echo "IDEALL_VERSION must be SemVer" >&2
  exit 2
}
[[ "${version_code}" =~ ^[1-9][0-9]*$ ]] || {
  echo "IDEALL_VERSION_CODE must be a positive integer" >&2
  exit 2
}

case "${profile}" in
  debug) cargo_profile=(); profile_dir="debug"; configuration="Debug" ;;
  release) cargo_profile=(--release); profile_dir="release"; configuration="Release" ;;
  *) echo "profile must be debug or release" >&2; exit 2 ;;
esac

case "${platform}" in
  android|android-bundle)
    command -v cargo-ndk >/dev/null || {
      echo "cargo-ndk is required: cargo install cargo-ndk" >&2
      exit 1
    }
    output_dir="${script_dir}/platforms/android/app/src/main/jniLibs"
    mkdir -p "${output_dir}"
    cd "${workspace_dir}"
    android_abis="${IDEALL_ANDROID_ABIS:-arm64-v8a}"
    IFS=',' read -r -a android_abi_values <<< "${android_abis}"
    cargo_ndk_targets=()
    for android_abi in "${android_abi_values[@]}"; do
      case "${android_abi}" in
        arm64-v8a)
          rustup target add aarch64-linux-android
          cargo_ndk_targets+=(-t "${android_abi}")
          ;;
        x86_64)
          rustup target add x86_64-linux-android
          cargo_ndk_targets+=(-t "${android_abi}")
          ;;
        *)
          echo "IDEALL_ANDROID_ABIS only supports arm64-v8a and x86_64" >&2
          exit 2
          ;;
      esac
    done
    cargo ndk \
      --platform 26 \
      "${cargo_ndk_targets[@]}" \
      -o "${output_dir}" \
      build --locked -p ideall-mobile "${cargo_profile[@]}"
    while IFS= read -r -d '' native_library; do
      case "${native_library}" in
        */libideall_mobile.so) ;;
        *) rm -f -- "${native_library}" ;;
      esac
    done < <(find "${output_dir}" -type f -name '*.so' -print0)
    for android_abi in "${android_abi_values[@]}"; do
      expected_library="${output_dir}/${android_abi}/libideall_mobile.so"
      [[ -s "${expected_library}" ]] || {
        echo "missing Android Rust library: ${expected_library}" >&2
        exit 1
      }
    done
    : "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME is required for Android builds}"
    llvm_nm="$(
      find "${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt" \
        -type f -path '*/bin/llvm-nm' -print -quit
    )"
    [[ -x "${llvm_nm}" ]] || {
      echo "unable to locate NDK llvm-nm under ${ANDROID_NDK_HOME}" >&2
      exit 1
    }
    required_jni_symbols=(
      "Java_com_jinziyou_ideall_IdeallNativeActivity_nativeOnTextInput"
      "Java_com_jinziyou_ideall_IdeallNativeActivity_nativeSetSafeAreaInsets"
    )
    for android_abi in "${android_abi_values[@]}"; do
      expected_library="${output_dir}/${android_abi}/libideall_mobile.so"
      exported_symbols="$("${llvm_nm}" -D --defined-only "${expected_library}")"
      for required_jni_symbol in "${required_jni_symbols[@]}"; do
        awk '{ print $NF }' <<<"${exported_symbols}" |
          grep -Fx "${required_jni_symbol}" >/dev/null || {
          echo "missing Android JNI export ${required_jni_symbol} in ${expected_library}" >&2
          exit 1
        }
      done
    done
    cd "${script_dir}/platforms/android"
    if [[ "${platform}" == "android-bundle" ]]; then
      gradle_task=":app:bundle${configuration}"
    else
      gradle_task=":app:assemble${configuration}"
    fi
    if [[ -x ./gradlew ]]; then
      ./gradlew "${gradle_task}"
    elif command -v gradle >/dev/null; then
      gradle "${gradle_task}"
    else
      echo "Gradle 8.9+ is required (or generate a wrapper in platforms/android)." >&2
      exit 1
    fi
    ;;
  ios-simulator|ios-device|ios-archive|ios-ipa)
    [[ "$(uname -s)" == "Darwin" ]] || {
      echo "iOS builds require macOS with Xcode." >&2
      exit 1
    }
    command -v xcodegen >/dev/null || {
      echo "XcodeGen is required: brew install xcodegen" >&2
      exit 1
    }
    if [[ "${platform}" == "ios-simulator" ]]; then
      rust_target="aarch64-apple-ios-sim"
      sdk="iphonesimulator"
      destination="generic/platform=iOS Simulator"
    else
      [[ -n "${IDEALL_DEVELOPMENT_TEAM:-}" ]] || {
        echo "IDEALL_DEVELOPMENT_TEAM is required for signed iOS device builds." >&2
        exit 1
      }
      rust_target="aarch64-apple-ios"
      sdk="iphoneos"
      destination="generic/platform=iOS"
    fi
    rustup target add "${rust_target}"
    cd "${workspace_dir}"
    export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"
    cargo build --locked -p ideall-mobile --target "${rust_target}" "${cargo_profile[@]}"
    cd "${script_dir}/platforms/ios"
    xcodegen generate
    xcode_action=(build)
    if [[ "${platform}" == "ios-archive" || "${platform}" == "ios-ipa" ]]; then
      xcode_action=(archive -archivePath "${script_dir}/platforms/ios/build/ideall.xcarchive")
    fi
    if [[ "${platform}" == "ios-simulator" ]]; then
      signing_allowed=NO
    else
      signing_allowed=YES
    fi
    xcodebuild \
      -project Ideall.xcodeproj \
      -scheme Ideall \
      -configuration "${configuration}" \
      -sdk "${sdk}" \
      -destination "${destination}" \
      -derivedDataPath "${script_dir}/platforms/ios/build" \
      IDEALL_RUST_PROFILE="${profile_dir}" \
      MARKETING_VERSION="${numeric_version}" \
      CURRENT_PROJECT_VERSION="${version_code}" \
      DEVELOPMENT_TEAM="${IDEALL_DEVELOPMENT_TEAM:-}" \
      CODE_SIGNING_ALLOWED="${signing_allowed}" \
      CODE_SIGN_STYLE="${IDEALL_IOS_CODE_SIGN_STYLE:-Automatic}" \
      PROVISIONING_PROFILE_SPECIFIER="${IDEALL_IOS_PROVISIONING_PROFILE_SPECIFIER:-}" \
      "${xcode_action[@]}"
    if [[ "${platform}" == "ios-ipa" ]]; then
      [[ -n "${IDEALL_IOS_PROVISIONING_PROFILE_SPECIFIER:-}" ]] || {
        echo "IDEALL_IOS_PROVISIONING_PROFILE_SPECIFIER is required to export an IPA." >&2
        exit 1
      }
      [[ "${IDEALL_DEVELOPMENT_TEAM}" != *$'\n'* \
        && "${IDEALL_IOS_PROVISIONING_PROFILE_SPECIFIER}" != *$'\n'* \
        && "${IDEALL_DEVELOPMENT_TEAM}" != *$'\r'* \
        && "${IDEALL_IOS_PROVISIONING_PROFILE_SPECIFIER}" != *$'\r'* ]] || {
        echo "iOS signing values must not contain newline characters." >&2
        exit 2
      }
      xml_escape() {
        printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
      }
      sed_replacement_escape() {
        sed -e 's/[\\&|]/\\&/g'
      }
      team_id="$(xml_escape "${IDEALL_DEVELOPMENT_TEAM}" | sed_replacement_escape)"
      profile_name="$(
        xml_escape "${IDEALL_IOS_PROVISIONING_PROFILE_SPECIFIER}" | sed_replacement_escape
      )"
      export_options="${script_dir}/platforms/ios/build/ExportOptions.plist"
      export_path="${script_dir}/platforms/ios/build/export"
      sed \
        -e "s|@IDEALL_TEAM_ID@|${team_id}|g" \
        -e "s|@IDEALL_PROFILE@|${profile_name}|g" \
        "${script_dir}/platforms/ios/ExportOptions.plist" > "${export_options}"
      plutil -lint "${export_options}" >/dev/null
      xcodebuild \
        -exportArchive \
        -archivePath "${script_dir}/platforms/ios/build/ideall.xcarchive" \
        -exportPath "${export_path}" \
        -exportOptionsPlist "${export_options}"
      ipa_outputs=("${export_path}"/*.ipa)
      [[ ${#ipa_outputs[@]} -eq 1 && -f "${ipa_outputs[0]}" ]] || {
        echo "expected exactly one exported IPA" >&2
        exit 1
      }
      cp "${ipa_outputs[0]}" \
        "${script_dir}/platforms/ios/build/ideall-native-${version}-ios-arm64.ipa"
    fi
    ;;
  *)
    echo "usage: $0 android|android-bundle|ios-simulator|ios-device|ios-archive|ios-ipa [debug|release]" >&2
    exit 2
    ;;
esac
