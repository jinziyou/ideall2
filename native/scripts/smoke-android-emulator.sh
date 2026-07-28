#!/usr/bin/env bash
set -euo pipefail

apk_path="${1:-}"
package_name="com.jinziyou.ideall"
activity_name="${package_name}/com.jinziyou.ideall.IdeallNativeActivity"
screenshot_path="${RUNNER_TEMP:-/tmp}/ideall-android-smoke.png"
window_dump_path="${RUNNER_TEMP:-/tmp}/ideall-android-window.xml"

[[ -n "${apk_path}" && -f "${apk_path}" ]] || {
  echo "usage: $0 <debug-apk>" >&2
  exit 2
}
command -v adb >/dev/null || {
  echo "adb is required" >&2
  exit 1
}

capture_exit_diagnostics() {
  local status=$?
  trap - EXIT
  adb shell uiautomator dump /sdcard/ideall-window.xml >/dev/null 2>&1 || true
  adb pull /sdcard/ideall-window.xml "${window_dump_path}" >/dev/null 2>&1 || true
  adb shell screencap -p /sdcard/ideall-android-smoke.png >/dev/null 2>&1 || true
  adb pull /sdcard/ideall-android-smoke.png "${screenshot_path}" >/dev/null 2>&1 || true
  exit "${status}"
}
trap capture_exit_diagnostics EXIT

assert_running() {
  adb shell pidof "${package_name}" | grep -E '[0-9]+' >/dev/null || {
    adb logcat -d -b all |
      grep -E -C 80 'com\.jinziyou\.ideall|ideall_mobile|must construct App' || true
    exit 1
  }
  adb shell dumpsys activity activities |
    grep -E "(mResumedActivity|topResumedActivity).*${package_name}" >/dev/null
  ! adb logcat -d -b crash |
      grep -E "${package_name}|ideall_mobile" >/dev/null
}

dismiss_system_anr_dialogs() {
  local attempt
  local window_xml
  for attempt in {1..3}; do
    adb shell uiautomator dump /sdcard/ideall-window.xml >/dev/null 2>&1 || true
    window_xml="$(adb shell cat /sdcard/ideall-window.xml 2>/dev/null || true)"
    if ! grep -F 'resource-id="android:id/aerr_wait"' <<<"${window_xml}" >/dev/null; then
      return 0
    fi
    echo "dismissing system ANR dialog before checking ideall input" >&2
    adb shell input tap "$((screen_width * 50 / 100))" "$((screen_height * 55 / 100))"
    sleep 2
  done
  echo "system ANR dialog remained visible" >&2
  return 1
}

wait_for_accessible_input() {
  local label="$1"
  local attempt
  local tap_y_percent
  local window_xml
  for attempt in {1..20}; do
    adb shell uiautomator dump /sdcard/ideall-window.xml >/dev/null 2>&1 || true
    window_xml="$(adb shell cat /sdcard/ideall-window.xml 2>/dev/null || true)"
    if grep -F 'resource-id="android:id/aerr_wait"' <<<"${window_xml}" >/dev/null; then
      dismiss_system_anr_dialogs
      continue
    fi
    if grep -F "content-desc=\"${label}\"" <<<"${window_xml}" |
      grep -F 'focused="true"' >/dev/null; then
      return 0
    fi
    case "${label}" in
      "标题")
        tap_y_percent=$((14 + ((attempt - 1) % 7) * 2))
        adb shell input tap "$((screen_width * 50 / 100))" \
          "$((screen_height * tap_y_percent / 100))"
        ;;
      "正文")
        tap_y_percent=$((28 + ((attempt - 1) % 3) * 8))
        adb shell input tap "$((screen_width * 50 / 100))" \
          "$((screen_height * tap_y_percent / 100))"
        ;;
    esac
    sleep 1
  done
  echo "timed out waiting for focused Android input: ${label}" >&2
  adb shell cat /sdcard/ideall-window.xml 2>/dev/null || true
  return 1
}

create_note_and_wait_for_body() {
  local attempt
  local tap_y_percent
  local window_xml
  for attempt in {1..14}; do
    adb shell uiautomator dump /sdcard/ideall-window.xml >/dev/null 2>&1 || true
    window_xml="$(adb shell cat /sdcard/ideall-window.xml 2>/dev/null || true)"
    if grep -F 'resource-id="android:id/aerr_wait"' <<<"${window_xml}" >/dev/null; then
      dismiss_system_anr_dialogs
      continue
    fi
    if grep -F 'content-desc="正文"' <<<"${window_xml}" |
      grep -F 'focused="true"' >/dev/null; then
      return 0
    fi
    tap_y_percent=$((4 + ((attempt - 1) % 7)))
    adb shell input tap "$((screen_width * 87 / 100))" \
      "$((screen_height * tap_y_percent / 100))"
    sleep 2
  done
  echo "timed out opening a new Android note" >&2
  adb shell cat /sdcard/ideall-window.xml 2>/dev/null || true
  return 1
}

screen_size="$(
  adb shell wm size |
    sed -n 's/.*Physical size: \([0-9][0-9]*x[0-9][0-9]*\).*/\1/p' |
    tail -1
)"
[[ "${screen_size}" =~ ^([0-9]+)x([0-9]+)$ ]] || {
  echo "unable to resolve emulator screen size: ${screen_size}" >&2
  exit 1
}
screen_width="${BASH_REMATCH[1]}"
screen_height="${BASH_REMATCH[2]}"

adb logcat -c
adb install -r "${apk_path}"
adb shell am start -W -n "${activity_name}"
sleep 5
dismiss_system_anr_dialogs
assert_running

# Create a note, then prove that GPUI focus reaches the real Android EditText
# bridge and accepts text for both multiline and single-line fields. Creating
# a note focuses its body, which also gives us a semantic readiness signal.
create_note_and_wait_for_body
adb shell input text "ideall-android-smoke-body"
adb shell input keyevent KEYCODE_BACK

adb shell input tap "$((screen_width * 50 / 100))" "$((screen_height * 16 / 100))"
wait_for_accessible_input "标题"
adb shell input text "ideall-android-smoke-title"
adb shell input keyevent KEYCODE_BACK
adb shell input swipe \
  "$((screen_width * 50 / 100))" \
  "$((screen_height * 70 / 100))" \
  "$((screen_width * 50 / 100))" \
  "$((screen_height * 30 / 100))" \
  350
assert_running

# Exercise configuration changes plus background/foreground delivery without
# allowing Android to silently recreate a crashed process.
initial_pid="$(adb shell pidof "${package_name}" | tr -d '\r')"
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 1
sleep 3
assert_running
[[ "$(adb shell pidof "${package_name}" | tr -d '\r')" == "${initial_pid}" ]] || {
  echo "Android process restarted during rotation" >&2
  exit 1
}

adb shell input keyevent KEYCODE_HOME
sleep 2
adb shell pidof "${package_name}" | grep -E '[0-9]+' >/dev/null
adb shell am start -W -n "${activity_name}"
sleep 3
assert_running
[[ "$(adb shell pidof "${package_name}" | tr -d '\r')" == "${initial_pid}" ]] || {
  echo "Android process restarted during background resume" >&2
  exit 1
}

adb shell settings put system user_rotation 0
adb shell screencap -p /sdcard/ideall-android-smoke.png
adb pull /sdcard/ideall-android-smoke.png "${screenshot_path}" >/dev/null
[[ -s "${screenshot_path}" ]] || {
  echo "Android smoke screenshot is empty" >&2
  exit 1
}
echo "Android interactive smoke passed; screenshot: ${screenshot_path}"
