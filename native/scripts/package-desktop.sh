#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_dir="$(cd "${script_dir}/.." && pwd)"
repository_dir="$(cd "${workspace_dir}/.." && pwd)"
version="${IDEALL_VERSION:-0.2.0}"
numeric_version="${version%%[-+]*}"

[[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || {
  echo "IDEALL_VERSION must be SemVer" >&2
  exit 2
}

stage_dir="${workspace_dir}/target/native-packages"
rm -rf "${stage_dir}"
mkdir -p "${stage_dir}"

copy_licenses() {
  local destination="$1"
  cp "${repository_dir}/LICENSE" "${destination}/LICENSE"
  cp "${workspace_dir}/THIRD_PARTY_LICENSES.md" "${destination}/THIRD_PARTY_LICENSES.md"
  cp "${workspace_dir}/README.md" "${destination}/README.md"
}

windows_path() {
  cygpath -w "$1"
}

sign_windows_file() {
  local target="$1"
  [[ -n "${IDEALL_WINDOWS_CERTIFICATE:-}" ]] || return 0
  [[ -n "${IDEALL_WINDOWS_CERTIFICATE_PASSWORD:-}" ]] || {
    echo "IDEALL_WINDOWS_CERTIFICATE_PASSWORD is required for Windows signing" >&2
    exit 1
  }
  IDEALL_WINDOWS_SIGN_TARGET="$(windows_path "${target}")"
  export IDEALL_WINDOWS_SIGN_TARGET
  powershell.exe -NoProfile -Command \
    '$tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1; if (-not $tool) { throw "signtool.exe not found" }; & $tool.FullName sign /fd SHA256 /f $env:IDEALL_WINDOWS_CERTIFICATE /p $env:IDEALL_WINDOWS_CERTIFICATE_PASSWORD /tr http://timestamp.digicert.com /td SHA256 $env:IDEALL_WINDOWS_SIGN_TARGET; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }'
  powershell.exe -NoProfile -Command \
    '$tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1; if (-not $tool) { throw "signtool.exe not found" }; & $tool.FullName verify /pa /v $env:IDEALL_WINDOWS_SIGN_TARGET; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }'
}

case "$(uname -s)" in
  Linux)
    case "$(uname -m)" in
      x86_64)
        package_arch="x86_64"
        debian_arch="amd64"
        rpm_arch="x86_64"
        ;;
      aarch64|arm64)
        package_arch="arm64"
        debian_arch="arm64"
        rpm_arch="aarch64"
        ;;
      *)
        echo "unsupported Linux architecture: $(uname -m)" >&2
        exit 1
        ;;
    esac
    command -v dpkg-deb >/dev/null || {
      echo "dpkg-deb is required to build the Linux installer" >&2
      exit 1
    }
    command -v rpmbuild >/dev/null || {
      echo "rpmbuild is required to build the Linux installer" >&2
      exit 1
    }
    binary="${workspace_dir}/target/release/ideall-desktop"
    [[ -x "${binary}" ]] || {
      echo "missing release binary: ${binary}" >&2
      exit 1
    }

    package="ideall-native-${version}-linux-${package_arch}"
    directory="${stage_dir}/${package}"
    mkdir -p "${directory}"
    cp "${binary}" "${directory}/ideall"
    copy_licenses "${directory}"
    tar -C "${stage_dir}" -czf "${stage_dir}/${package}.tar.gz" "${package}"

    deb_root="${stage_dir}/debian-root"
    install -Dm0755 "${binary}" "${deb_root}/usr/bin/ideall"
    install -Dm0644 "${script_dir}/resources/linux/com.jinziyou.ideall.desktop" \
      "${deb_root}/usr/share/applications/com.jinziyou.ideall.desktop"
    install -Dm0644 "${script_dir}/resources/icons/icon-256.png" \
      "${deb_root}/usr/share/icons/hicolor/256x256/apps/com.jinziyou.ideall.png"
    install -Dm0644 "${repository_dir}/LICENSE" "${deb_root}/usr/share/doc/ideall/LICENSE"
    install -Dm0644 "${workspace_dir}/THIRD_PARTY_LICENSES.md" \
      "${deb_root}/usr/share/doc/ideall/THIRD_PARTY_LICENSES.md"
    install -Dm0644 "${workspace_dir}/README.md" "${deb_root}/usr/share/doc/ideall/README.md"
    mkdir -p "${deb_root}/DEBIAN"
    sed \
      -e "s/@IDEALL_VERSION@/${version}/g" \
      -e "s/@IDEALL_DEBIAN_ARCH@/${debian_arch}/g" \
      "${script_dir}/resources/linux/debian-control" > "${deb_root}/DEBIAN/control"
    dpkg-deb --root-owner-group --build "${deb_root}" "${stage_dir}/${package}.deb"

    rpm_topdir="${stage_dir}/rpmbuild"
    mkdir -p "${rpm_topdir}/BUILD" "${rpm_topdir}/BUILDROOT" "${rpm_topdir}/RPMS" \
      "${rpm_topdir}/SOURCES" "${rpm_topdir}/SPECS" "${rpm_topdir}/SRPMS"
    cp "${binary}" "${rpm_topdir}/SOURCES/ideall"
    cp "${script_dir}/resources/linux/com.jinziyou.ideall.desktop" "${rpm_topdir}/SOURCES/"
    cp "${script_dir}/resources/icons/icon-256.png" \
      "${rpm_topdir}/SOURCES/com.jinziyou.ideall.png"
    cp "${repository_dir}/LICENSE" "${rpm_topdir}/SOURCES/LICENSE"
    cp "${workspace_dir}/THIRD_PARTY_LICENSES.md" "${rpm_topdir}/SOURCES/THIRD_PARTY_LICENSES.md"
    cp "${workspace_dir}/README.md" "${rpm_topdir}/SOURCES/README.md"
    sed \
      -e "s/@IDEALL_VERSION@/${numeric_version}/g" \
      -e "s/@IDEALL_RPM_ARCH@/${rpm_arch}/g" \
      "${script_dir}/resources/linux/ideall.spec" > "${rpm_topdir}/SPECS/ideall.spec"
    rpmbuild -bb --define "_topdir ${rpm_topdir}" "${rpm_topdir}/SPECS/ideall.spec"
    rpm_outputs=("${rpm_topdir}/RPMS/${rpm_arch}"/*.rpm)
    [[ ${#rpm_outputs[@]} -eq 1 && -f "${rpm_outputs[0]}" ]] || {
      echo "expected exactly one RPM output" >&2
      exit 1
    }
    cp "${rpm_outputs[0]}" "${stage_dir}/${package}.rpm"
    ;;
  Darwin)
    case "$(uname -m)" in
      arm64) package_arch="arm64" ;;
      x86_64) package_arch="x86_64" ;;
      *)
        echo "unsupported macOS architecture: $(uname -m)" >&2
        exit 1
        ;;
    esac
    binary="${workspace_dir}/target/release/ideall-desktop"
    [[ -x "${binary}" ]] || {
      echo "missing release binary: ${binary}" >&2
      exit 1
    }
    package="ideall-native-${version}-macos-${package_arch}"
    app="${stage_dir}/ideall.app"
    mkdir -p "${app}/Contents/MacOS" "${app}/Contents/Resources"
    cp "${binary}" "${app}/Contents/MacOS/ideall"
    cp "${script_dir}/resources/icons/icon.icns" "${app}/Contents/Resources/icon.icns"
    copy_licenses "${app}/Contents/Resources"
    sed \
      -e "s/@IDEALL_NUMERIC_VERSION@/${numeric_version}/g" \
      "${script_dir}/resources/macos-Info.plist" > "${app}/Contents/Info.plist"
    if [[ -n "${IDEALL_MACOS_SIGNING_IDENTITY:-}" ]]; then
      codesign --force --deep --options runtime --timestamp \
        --sign "${IDEALL_MACOS_SIGNING_IDENTITY}" "${app}"
      codesign --verify --deep --strict --verbose=2 "${app}"
    fi
    ditto -c -k --keepParent "${app}" "${stage_dir}/${package}.zip"
    notary_values="${IDEALL_APPLE_ID:-}${IDEALL_APPLE_TEAM_ID:-}${IDEALL_APPLE_APP_PASSWORD:-}"
    if [[ -n "${notary_values}" ]]; then
      [[ -n "${IDEALL_MACOS_SIGNING_IDENTITY:-}" \
        && -n "${IDEALL_APPLE_ID:-}" \
        && -n "${IDEALL_APPLE_TEAM_ID:-}" \
        && -n "${IDEALL_APPLE_APP_PASSWORD:-}" ]] || {
        echo "macOS notarization requires signing identity, Apple ID, team ID and app password" >&2
        exit 1
      }
      xcrun notarytool submit "${stage_dir}/${package}.zip" \
        --apple-id "${IDEALL_APPLE_ID}" \
        --team-id "${IDEALL_APPLE_TEAM_ID}" \
        --password "${IDEALL_APPLE_APP_PASSWORD}" \
        --wait
      xcrun stapler staple "${app}"
      rm -f "${stage_dir}/${package}.zip"
      ditto -c -k --keepParent "${app}" "${stage_dir}/${package}.zip"
    fi

    dmg_source="${stage_dir}/dmg-source"
    mkdir -p "${dmg_source}"
    ditto "${app}" "${dmg_source}/ideall.app"
    hdiutil create -volname "ideall" -srcfolder "${dmg_source}" -ov -format UDZO \
      "${stage_dir}/${package}.dmg"
    if [[ -n "${notary_values}" ]]; then
      xcrun notarytool submit "${stage_dir}/${package}.dmg" \
        --apple-id "${IDEALL_APPLE_ID}" \
        --team-id "${IDEALL_APPLE_TEAM_ID}" \
        --password "${IDEALL_APPLE_APP_PASSWORD}" \
        --wait
      xcrun stapler staple "${stage_dir}/${package}.dmg"
    fi
    ;;
  MINGW*|MSYS*|CYGWIN*)
    binary="${workspace_dir}/target/release/ideall-desktop.exe"
    [[ -x "${binary}" || -f "${binary}" ]] || {
      echo "missing release binary: ${binary}" >&2
      exit 1
    }
    command -v dotnet >/dev/null || {
      echo "dotnet is required to run the pinned WiX tool" >&2
      exit 1
    }
    makensis="$(command -v makensis.exe || command -v makensis || true)"
    if [[ -z "${makensis}" && -x "/c/Program Files (x86)/NSIS/makensis.exe" ]]; then
      makensis="/c/Program Files (x86)/NSIS/makensis.exe"
    fi
    [[ -n "${makensis}" ]] || {
      echo "makensis is required to build the Windows installer" >&2
      exit 1
    }

    package="ideall-native-${version}-windows-x86_64"
    directory="${stage_dir}/${package}"
    mkdir -p "${directory}"
    cp "${binary}" "${directory}/ideall.exe"
    copy_licenses "${directory}"
    sign_windows_file "${directory}/ideall.exe"

    windows_directory="$(windows_path "${directory}")"
    windows_archive="$(windows_path "${stage_dir}/${package}.zip")"
    powershell.exe -NoProfile -Command \
      "Compress-Archive -LiteralPath '${windows_directory}' -DestinationPath '${windows_archive}' -Force"

    msi="${stage_dir}/${package}.msi"
    (
      cd "${workspace_dir}"
      dotnet tool run wix -- build -arch x64 \
        -d "IdeallVersion=${numeric_version}" \
        -d "IdeallBinary=$(windows_path "${directory}/ideall.exe")" \
        -d "IdeallLicense=$(windows_path "${directory}/LICENSE")" \
        -d "IdeallThirdPartyLicenses=$(windows_path "${directory}/THIRD_PARTY_LICENSES.md")" \
        -d "IdeallReadme=$(windows_path "${directory}/README.md")" \
        -d "IdeallIcon=$(windows_path "${script_dir}/resources/icons/icon.ico")" \
        -o "$(windows_path "${msi}")" \
        "$(windows_path "${script_dir}/resources/windows/ideall.wxs")"
    )

    setup="${stage_dir}/${package}-setup.exe"
    "${makensis}" \
      -DIDEALL_VERSION="${version}" \
      -DIDEALL_VERSION_NUMERIC="${numeric_version}" \
      -DIDEALL_BINARY="$(windows_path "${directory}/ideall.exe")" \
      -DIDEALL_LICENSE="$(windows_path "${directory}/LICENSE")" \
      -DIDEALL_THIRD_PARTY_LICENSES="$(windows_path "${directory}/THIRD_PARTY_LICENSES.md")" \
      -DIDEALL_README="$(windows_path "${directory}/README.md")" \
      -DIDEALL_ICON="$(windows_path "${script_dir}/resources/icons/icon.ico")" \
      -DIDEALL_OUTPUT="$(windows_path "${setup}")" \
      "$(windows_path "${script_dir}/resources/windows/ideall.nsi")"
    sign_windows_file "${msi}"
    sign_windows_file "${setup}"
    ;;
  *)
    echo "unsupported desktop packaging host: $(uname -s)" >&2
    exit 1
    ;;
esac

(
  cd "${stage_dir}"
  candidates=(./*.tar.gz ./*.zip ./*.deb ./*.rpm ./*.dmg ./*.msi ./*-setup.exe)
  artifacts=()
  for candidate in "${candidates[@]}"; do
    [[ -f "${candidate}" ]] && artifacts+=("${candidate}")
  done
  [[ ${#artifacts[@]} -gt 0 ]] || {
    echo "desktop packaging produced no artifacts" >&2
    exit 1
  }
  if command -v sha256sum >/dev/null; then
    sha256sum "${artifacts[@]}" > SHA256SUMS
  else
    shasum -a 256 "${artifacts[@]}" > SHA256SUMS
  fi
)
test -s "${stage_dir}/SHA256SUMS"
find "${stage_dir}" -maxdepth 1 -type f -print
