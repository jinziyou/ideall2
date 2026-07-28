#!/usr/bin/env bash
set -euo pipefail

kind="${1:-}"
artifact="${2:-}"

[[ -n "${kind}" && -n "${artifact}" ]] || {
  echo "usage: $0 desktop|android-apk|android-aab|ios-app|ios-archive|ios-ipa ARTIFACT" >&2
  exit 2
}
[[ -e "${artifact}" ]] || {
  echo "artifact does not exist: ${artifact}" >&2
  exit 1
}

case "${kind}" in
  desktop)
    case "${artifact}" in
      *.tar.gz)
        tar -tzf "${artifact}" | grep -E '/ideall$' >/dev/null
        ;;
      *.zip)
        unzip -Z1 "${artifact}" | \
          grep -E '(^|[/\\])(ideall\.exe|ideall\.app[/\\]Contents[/\\]MacOS[/\\]ideall)$' \
          >/dev/null
        ;;
      *.deb)
        command -v dpkg-deb >/dev/null || {
          echo "dpkg-deb is required to verify a Debian package" >&2
          exit 1
        }
        dpkg-deb --info "${artifact}" >/dev/null
        dpkg-deb --contents "${artifact}" | grep -E '\./usr/bin/ideall$' >/dev/null
        dpkg-deb --contents "${artifact}" | \
          grep -E '\./usr/share/applications/com\.jinziyou\.ideall\.desktop$' >/dev/null
        ;;
      *.rpm)
        command -v rpm >/dev/null || {
          echo "rpm is required to verify an RPM package" >&2
          exit 1
        }
        rpm -qpl "${artifact}" | grep -E '^/usr/bin/ideall$' >/dev/null
        rpm -qpl "${artifact}" | \
          grep -E '^/usr/share/applications/com\.jinziyou\.ideall\.desktop$' >/dev/null
        ;;
      *.dmg)
        command -v hdiutil >/dev/null || {
          echo "hdiutil is required to verify a disk image" >&2
          exit 1
        }
        hdiutil verify "${artifact}" >/dev/null
        mount_dir="$(mktemp -d)"
        cleanup_dmg() {
          hdiutil detach "${mount_dir}" -quiet >/dev/null 2>&1 || true
          rmdir "${mount_dir}" >/dev/null 2>&1 || true
        }
        trap cleanup_dmg EXIT
        hdiutil attach -readonly -nobrowse -mountpoint "${mount_dir}" "${artifact}" >/dev/null
        [[ -x "${mount_dir}/ideall.app/Contents/MacOS/ideall" ]]
        cleanup_dmg
        trap - EXIT
        ;;
      *.msi)
        command -v dotnet >/dev/null || {
          echo "dotnet and the pinned WiX tool are required to verify an MSI" >&2
          exit 1
        }
        command -v cygpath >/dev/null || {
          echo "MSI verification must run from a Windows shell" >&2
          exit 1
        }
        verify_dir="$(mktemp -d)"
        script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
        workspace_dir="$(cd "${script_dir}/.." && pwd)"
        (
          cd "${workspace_dir}"
          dotnet tool run wix -- msi decompile \
            -o "$(cygpath -w "${verify_dir}/package.wxs")" \
            "$(cygpath -w "$(cd "$(dirname "${artifact}")" && pwd)/$(basename "${artifact}")")" \
            >/dev/null
        )
        grep -i 'ideall\.exe' "${verify_dir}/package.wxs" >/dev/null
        rm -rf "${verify_dir}"
        ;;
      *-setup.exe)
        seven_zip="$(command -v 7z.exe || command -v 7z || true)"
        [[ -n "${seven_zip}" ]] || {
          echo "7-Zip is required to verify an NSIS installer" >&2
          exit 1
        }
        "${seven_zip}" l -ba "${artifact}" | grep -iE 'ideall\.exe$' >/dev/null
        ;;
      *)
        echo "unsupported desktop artifact: ${artifact}" >&2
        exit 2
        ;;
    esac
    ;;
  android-apk)
    unzip -tq "${artifact}" >/dev/null
    unzip -Z1 "${artifact}" | grep -E '^lib/arm64-v8a/libideall_mobile\.so$' >/dev/null
    unzip -Z1 "${artifact}" | grep -E '^AndroidManifest\.xml$' >/dev/null
    ;;
  android-aab)
    unzip -tq "${artifact}" >/dev/null
    unzip -Z1 "${artifact}" | \
      grep -E '^base/lib/arm64-v8a/libideall_mobile\.so$' >/dev/null
    unzip -Z1 "${artifact}" | grep -E '^base/manifest/AndroidManifest\.xml$' >/dev/null
    ;;
  ios-app)
    [[ -d "${artifact}" ]]
    [[ -x "${artifact}/ideall" ]]
    plutil -lint "${artifact}/Info.plist" >/dev/null
    [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${artifact}/Info.plist")" == \
      "com.jinziyou.ideall" ]]
    ;;
  ios-archive)
    [[ -d "${artifact}" ]]
    app="${artifact}/Products/Applications/ideall.app"
    [[ -x "${app}/ideall" ]]
    codesign --verify --deep --strict "${app}"
    ;;
  ios-ipa)
    unzip -tq "${artifact}" >/dev/null
    unzip -Z1 "${artifact}" | grep -E '^Payload/[^/]+\.app/ideall$' >/dev/null
    ;;
  *)
    echo "unknown artifact kind: ${kind}" >&2
    exit 2
    ;;
esac

echo "verified ${kind}: ${artifact}"
