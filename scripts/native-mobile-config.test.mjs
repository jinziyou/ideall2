import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { spawnSync } from "node:child_process"
import test from "node:test"

const IOS_PROJECT = new URL(
  "../native/apps/ideall-mobile/platforms/ios/project.yml",
  import.meta.url,
)
const IOS_MAIN = new URL("../native/apps/ideall-mobile/platforms/ios/main.m", import.meta.url)
const IOS_INFO_PLIST = new URL(
  "../native/apps/ideall-mobile/platforms/ios/Info.plist",
  import.meta.url,
)
const ANDROID_APP_BUILD = new URL(
  "../native/apps/ideall-mobile/platforms/android/app/build.gradle.kts",
  import.meta.url,
)
const ANDROID_MANIFEST = new URL(
  "../native/apps/ideall-mobile/platforms/android/app/src/main/AndroidManifest.xml",
  import.meta.url,
)
const ANDROID_PLATFORM_VIEW = new URL(
  "../native/apps/ideall-mobile/platforms/android/app/src/main/java/dev/gpui/mobile/GpuiPlatformView.java",
  import.meta.url,
)
const ANDROID_ACTIVITY = new URL(
  "../native/apps/ideall-mobile/platforms/android/app/src/main/java/com/jinziyou/ideall/IdeallNativeActivity.java",
  import.meta.url,
)
const MOBILE_BUILD_SCRIPT = new URL("../native/apps/ideall-mobile/build-mobile.sh", import.meta.url)
const MOBILE_MANIFEST = new URL("../native/apps/ideall-mobile/Cargo.toml", import.meta.url)
const MOBILE_LIB = new URL("../native/apps/ideall-mobile/src/lib.rs", import.meta.url)
const MOBILE_VIEW = new URL("../native/apps/ideall-mobile/src/mobile/view.rs", import.meta.url)
const VENDORED_GPUI_README = new URL("../native/vendor/gpui-mobile/README.md", import.meta.url)
const VENDORED_GPUI_SOURCE = new URL("../native/vendor/gpui-mobile/upstream.env", import.meta.url)
const VENDORED_GPUI_MANIFEST = new URL("../native/vendor/gpui-mobile/Cargo.toml", import.meta.url)
const VENDORED_GPUI_CHECKSUMS = new URL(
  "../native/vendor/gpui-mobile/PATCHED_FILES.sha256",
  import.meta.url,
)
const VENDORED_GPUI_VERIFY = new URL(
  "../native/scripts/verify-gpui-mobile-vendor.sh",
  import.meta.url,
)
const NATIVE_CARGO_LOCK = new URL("../native/Cargo.lock", import.meta.url)
const IOS_UI_SMOKE = new URL(
  "../native/apps/ideall-mobile/platforms/ios/IdeallUITests/IdeallSmokeTests.swift",
  import.meta.url,
)
const ANDROID_SMOKE = new URL("../native/scripts/smoke-android-emulator.sh", import.meta.url)
const IOS_SMOKE = new URL("../native/scripts/smoke-ios-simulator.sh", import.meta.url)
const RUST_WORKFLOW = new URL("../.github/workflows/rust.yml", import.meta.url)
const VENDORED_GPUI_IOS_FFI = new URL(
  "../native/vendor/gpui-mobile/src/ios/ffi.rs",
  import.meta.url,
)
const VENDORED_GPUI_IOS_WINDOW = new URL(
  "../native/vendor/gpui-mobile/src/ios/window.rs",
  import.meta.url,
)
const VENDORED_GPUI_ANDROID_JNI = new URL(
  "../native/vendor/gpui-mobile/src/android/jni.rs",
  import.meta.url,
)
const VENDORED_GPUI_LIB = new URL("../native/vendor/gpui-mobile/src/lib.rs", import.meta.url)

test("iOS XcodeGen 工程为模拟器和真机选择对应的 Rust 静态库", () => {
  const project = readFileSync(IOS_PROJECT, "utf8")
  const infoPlist = readFileSync(IOS_INFO_PLIST, "utf8")
  const buildScript = readFileSync(MOBILE_BUILD_SCRIPT, "utf8")
  const appTarget = project.slice(
    project.indexOf("  Ideall:\n"),
    project.indexOf("  IdeallUITests:\n"),
  )
  const settingsBlocks = appTarget.match(/^    settings:$/gm) ?? []
  const deploymentTarget = project.match(/^\s*iOS:\s*"([^"]+)"\s*$/m)?.[1]

  assert.equal(settingsBlocks.length, 1, "target must have exactly one settings block")
  assert.ok(deploymentTarget, "iOS deployment target must be declared")
  assert.ok(
    buildScript.includes(
      `IPHONEOS_DEPLOYMENT_TARGET="\${IPHONEOS_DEPLOYMENT_TARGET:-${deploymentTarget}}"`,
    ),
    "Rust and Xcode must use the same iOS deployment target",
  )
  assert.match(
    project,
    /^        "IDEALL_STATIC_LIB\[sdk=iphonesimulator\*\]": .*aarch64-apple-ios-sim/m,
  )
  assert.match(project, /^        "IDEALL_STATIC_LIB\[sdk=iphoneos\*\]": .*aarch64-apple-ios\//m)
  assert.match(project, /^          - -framework CoreMedia$/m)
  assert.match(project, /^          - -framework AVFoundation$/m)
  assert.doesNotMatch(project, /^      settings:$/m, "build settings must not be nested")
  assert.match(project, /^  IdeallUITests:$/m)
  assert.match(project, /^    type: bundle\.ui-testing$/m)
  assert.match(project, /^        - IdeallUITests$/m)
  assert.match(infoPlist, /<key>UILaunchScreen<\/key>\s*<dict\/>/)
})

test("iOS 宿主在 UIKit 激活后启动 GPUI 并串行化状态通知", () => {
  const main = readFileSync(IOS_MAIN, "utf8")
  const mobileLib = readFileSync(MOBILE_LIB, "utf8")
  const mobileView = readFileSync(MOBILE_VIEW, "utf8")

  const didFinish = main.slice(
    main.indexOf("didFinishLaunchingWithOptions:"),
    main.indexOf("- (void)installTextInputBridgeIfNeeded"),
  )
  assert.doesNotMatch(didFinish, /gpui_ios_run_demo/)
  assert.match(main, /startGpuiIfNeeded[\s\S]*?gpui_ios_run_demo\(\)/)
  assert.match(main, /IdeallInstallCrashDiagnosticsIfRequested/)
  assert.match(main, /getenv\("IDEALL_CRASH_DIAGNOSTICS"\)/)
  assert.match(main, /gpuiActiveNotificationInProgress/)
  assert.match(main, /gpuiActiveNotificationPending/)
  assert.match(main, /gpuiFrameInProgress/)
  assert.match(
    main,
    /renderFrame[\s\S]*?gpuiFrameInProgress\) return;[\s\S]*?gpui_ios_request_frame/,
  )
  assert.match(main, /dispatch_async\(dispatch_get_main_queue\(\)/)
  assert.match(main, /applicationDidBecomeActive:[\s\S]*?\[self startGpuiIfNeeded\]/)
  assert.match(
    main,
    /applicationWillResignActive:[\s\S]*?\[self scheduleGpuiApplicationActive:NO\]/,
  )
  assert.doesNotMatch(main, /gpui_ios_will_enter_foreground\(NULL\)/)
  assert.doesNotMatch(main, /gpui_ios_did_enter_background\(NULL\)/)
  assert.match(main, /input\.isAccessibilityElement = YES/)
  assert.match(main, /input\.accessibilityIdentifier = fieldLabel/)
  assert.match(main, /\[rootView addSubview:input\]/)
  assert.match(main, /getenv\("IDEALL_UI_TESTING"\)/)
  assert.match(main, /containsObject:@"-IDEALLUITesting"/)
  assert.match(main, /rootView\.safeAreaInsets\.top \+ 4/)
  assert.match(main, /input\.alpha = isUITesting \? 1\.0 : 0\.01/)
  assert.match(main, /gpui_ios_get_native_view\(self\.gpuiWindow\)/)
  assert.match(main, /requestedSmokeAction/)
  assert.match(main, /indexOfObject:@"-IDEALLSmokeAction"/)
  assert.match(main, /indexOfObject:@"-IDEALLSmokeText"/)
  assert.match(main, /if \(injectedSmokeText\) \[self sendTextInputBridgeState\]/)
  assert.match(main, /ideall_mobile_ios_ui_test_action\(smokeAction\)/)
  assert.match(mobileLib, /pub extern "C" fn ideall_mobile_ios_ui_test_action\(action: u8\)/)
  assert.match(mobileLib, /std::env::var\("IDEALL_UI_TESTING"\)/)
  assert.match(mobileLib, /IOS_UI_TEST_ACTION\.store\(action/)
  assert.match(mobileLib, /fn drain_ui_test_action\(/)
  assert.match(mobileView, /self\.drain_ui_test_action\(cx\)/)
})

test("gpui-mobile 快照固定来源并在 Rust 边界串行化 iOS 帧泵", () => {
  const manifest = readFileSync(MOBILE_MANIFEST, "utf8")
  const provenance = readFileSync(VENDORED_GPUI_README, "utf8")
  const source = readFileSync(VENDORED_GPUI_SOURCE, "utf8")
  const vendorManifest = readFileSync(VENDORED_GPUI_MANIFEST, "utf8")
  const checksums = readFileSync(VENDORED_GPUI_CHECKSUMS, "utf8")
  const ffi = readFileSync(VENDORED_GPUI_IOS_FFI, "utf8")
  const window = readFileSync(VENDORED_GPUI_IOS_WINDOW, "utf8")
  const androidJni = readFileSync(VENDORED_GPUI_ANDROID_JNI, "utf8")
  const gpuiMobileLib = readFileSync(VENDORED_GPUI_LIB, "utf8")

  assert.match(manifest, /gpui-mobile = \{ path = "\.\.\/\.\.\/vendor\/gpui-mobile"/)
  assert.match(provenance, /1d3ec2a1d14a63b74d1f4269340441d4eeada27a/)
  assert.match(source, /GPUI_MOBILE_UPSTREAM_REVISION=1d3ec2a1d14a63b74d1f4269340441d4eeada27a/)
  assert.match(source, /GPUI_MOBILE_GPUI_REVISION=74798c68d5c63d31e2ccca5c8f5ec0a02c90679c/)
  assert.doesNotMatch(vendorManifest, /5688167d224b5eca54875d49afb8bfd73a07915a/)
  assert.match(vendorManifest, /rev = "74798c68d5c63d31e2ccca5c8f5ec0a02c90679c"/)
  assert.match(checksums, /native\/vendor\/gpui-mobile\/src\/ios\/ffi\.rs/)
  assert.doesNotMatch(readFileSync(NATIVE_CARGO_LOCK, "utf8"), /zed-industries\/wgpu\.git/)
  assert.match(readFileSync(NATIVE_CARGO_LOCK, "utf8"), /name = "wgpu"\nversion = "29\.0\.4"/)
  assert.match(ffi, /window\.begin_frame\(\)/)
  assert.match(ffi, /request_frame_callback\.try_borrow_mut\(\)/)
  assert.match(ffi, /Application::with_platform\(platform\)\.run_embedded/)
  assert.match(ffi, /\*application\.0\.get\(\) = Some\(handle\)/)
  assert.match(ffi, /pub extern "C" fn gpui_ios_get_native_view\(/)
  assert.match(window, /frame_in_progress: AtomicBool/)
  assert.match(window, /momentum_scroller\.try_borrow_mut\(\)/)
  assert.match(androidJni, /Mutex<Option<AndroidApp>>/)
  assert.match(androidJni, /pub fn clear_platform\(/)
  assert.match(androidJni, /pub fn update_safe_area_insets\(/)
  assert.match(androidJni, /pub fn safe_area_insets_logical\(/)
  assert.doesNotMatch(androidJni, /static PLATFORM: OnceLock/)
  assert.match(gpuiMobileLib, /android::jni::safe_area_insets_logical\(\)/)

  const result = spawnSync("bash", [VENDORED_GPUI_VERIFY.pathname], {
    cwd: new URL("..", import.meta.url),
    encoding: "utf8",
  })
  assert.equal(result.status, 0, result.stderr || result.stdout)
})

test("Android Rust NDK API 与应用 minSdk 保持一致", () => {
  const appBuild = readFileSync(ANDROID_APP_BUILD, "utf8")
  const buildScript = readFileSync(MOBILE_BUILD_SCRIPT, "utf8")
  const minSdk = appBuild.match(/^\s*minSdk\s*=\s*(\d+)\s*$/m)?.[1]

  assert.ok(minSdk, "Android minSdk must be declared")
  assert.match(buildScript, new RegExp(`--platform\\s+${minSdk}\\b`))
  assert.match(buildScript, /llvm-nm/)
  assert.match(buildScript, /grep -Fx "\$\{required_jni_symbol\}"/)
  assert.match(buildScript, /Java_com_jinziyou_ideall_IdeallNativeActivity_nativeSetSafeAreaInsets/)
  assert.match(buildScript, /Java_com_jinziyou_ideall_IdeallNativeActivity_nativeOnTextInput/)
})

test("Android 壳完整承接 GPUI 生命周期与 WebView 平台桥", () => {
  const mobileLib = readFileSync(MOBILE_LIB, "utf8")
  const manifest = readFileSync(ANDROID_MANIFEST, "utf8")
  const platformView = readFileSync(ANDROID_PLATFORM_VIEW, "utf8")
  const activity = readFileSync(ANDROID_ACTIVITY, "utf8")
  const configChanges = manifest.match(/android:configChanges="([^"]+)"/)?.[1]?.split("|")

  assert.deepEqual(configChanges, [
    "colorMode",
    "density",
    "fontScale",
    "fontWeightAdjustment",
    "grammaticalGender",
    "keyboard",
    "keyboardHidden",
    "layoutDirection",
    "locale",
    "mcc",
    "mnc",
    "navigation",
    "orientation",
    "screenLayout",
    "screenSize",
    "smallestScreenSize",
    "touchscreen",
    "uiMode",
  ])
  assert.match(platformView, /public static boolean createView\(/)
  assert.match(platformView, /public static void pauseAll\(\)/)
  assert.match(platformView, /public static void resumeAll\(\)/)
  assert.match(platformView, /public static void disposeAll\(\)/)
  assert.match(platformView, /settings\.setAllowFileAccess\(false\)/)
  assert.match(platformView, /settings\.setAllowContentAccess\(false\)/)
  assert.match(activity, /setOnApplyWindowInsetsListener/)
  assert.match(activity, /WindowInsets\.Type\.systemBars\(\)/)
  assert.match(activity, /WindowInsets\.Type\.displayCutout\(\)/)
  assert.match(activity, /nativeSetSafeAreaInsets/)
  assert.match(activity, /public void onIdeallNativeReady\(\)/)
  assert.match(mobileLib, /fn register_android_native_bridges\(/)
  assert.match(mobileLib, /env\.register_native_methods\(&activity_class, &methods\)/)
  assert.match(mobileLib, /jni::jni_str!\("onIdeallNativeReady"\)/)
  assert.ok(
    mobileLib.indexOf("register_android_native_bridges(&app)") <
      mobileLib.indexOf("gpui_mobile::android::jni::init_platform(&app)"),
    "native methods must be registered before GPUI initializes the Android platform",
  )
})

test("移动 CI 驱动真实输入、旋转和前后台恢复", () => {
  const iosTest = readFileSync(IOS_UI_SMOKE, "utf8")
  const androidSmoke = readFileSync(ANDROID_SMOKE, "utf8")
  const iosSmoke = readFileSync(IOS_SMOKE, "utf8")
  const workflow = readFileSync(RUST_WORKFLOW, "utf8")

  assert.match(iosTest, /waitForInput\("正文"/)
  assert.match(iosTest, /waitForInput\("标题"/)
  assert.match(iosTest, /app\.textViews\[label\]/)
  assert.match(iosTest, /launchEnvironment\["IDEALL_UI_TESTING"\] = "1"/)
  assert.match(iosTest, /app\.launchArguments = \["-IDEALLUITesting"\]/)
  assert.match(iosTest, /"-IDEALLSmokeAction"/)
  assert.match(iosTest, /"-IDEALLSmokeText"/)
  assert.match(iosTest, /smokeText: "ideall iOS smoke body/)
  assert.match(iosTest, /smokeText: "ideall iOS smoke title"/)
  assert.match(iosTest, /let deviceFrame = deviceWindow\.frame/)
  assert.match(iosTest, /assertFullScreen\(window\.frame, matches: deviceFrame\)/)
  assert.match(iosTest, /relaunch\(app, smokeAction: 2, smokeText:/)
  assert.match(iosTest, /relaunch\(app, smokeAction: 3, smokeText:/)
  assert.match(iosTest, /contains\("second line"\)/)
  assert.match(iosTest, /waitForAutosave\(\)/)
  assert.match(iosTest, /XCUIApplication\(bundleIdentifier: "com\.apple\.springboard"\)/)
  assert.doesNotMatch(iosTest, /app\.coordinate\(withNormalizedOffset:/)
  assert.doesNotMatch(iosTest, /\.coordinate\(withNormalizedOffset:/)
  assert.match(iosTest, /XCUIDevice\.shared\.orientation = \.landscapeLeft/)
  assert.match(iosTest, /XCUIDevice\.shared\.press\(\.home\)/)
  assert.match(iosTest, /app\.activate\(\)/)
  assert.match(androidSmoke, /wait_for_accessible_input "标题"/)
  assert.match(androidSmoke, /create_note_and_wait_for_body/)
  assert.match(androidSmoke, /tap_y_percent=\$\(\(14 \+ \(\(attempt - 1\) % 7\) \* 2\)\)/)
  assert.match(androidSmoke, /tap_y_percent=\$\(\(4 \+ \(\(attempt - 1\) % 7\)\)\)/)
  assert.match(androidSmoke, /screen_height \* tap_y_percent \/ 100/)
  assert.match(androidSmoke, /android:id\/aerr_wait/)
  assert.match(androidSmoke, /dismissing system ANR dialog/)
  assert.match(androidSmoke, /trap capture_exit_diagnostics EXIT/)
  assert.match(androidSmoke, /sleep 5\n+dismiss_system_anr_dialogs\n+assert_running/)
  assert.match(androidSmoke, /settings put system user_rotation 1/)
  assert.match(androidSmoke, /KEYCODE_HOME/)
  assert.match(iosSmoke, /xcodebuild[\s\S]*?-resultBundlePath[\s\S]*?test/)
  assert.match(workflow, /uses: \.\/\.github\/actions\/setup-cargo-ndk/)
  assert.match(workflow, /uses: \.\/\.github\/actions\/setup-xcodegen/)
  assert.match(workflow, /disk-size: 2048M/)
  assert.match(workflow, /Upload Android preview APK and AAB\n\s+if: always\(\)/)
  assert.match(workflow, /bash native\/scripts\/smoke-android-emulator\.sh "\$\(find [^\n]+\)"/)
  assert.doesNotMatch(workflow, /smoke-android-emulator\.sh \\\s*\n/)
  assert.match(workflow, /bash scripts\/smoke-ios-simulator\.sh/)
  assert.match(workflow, /CARGO_PROFILE_RELEASE_DEBUG=0/)
  assert.match(workflow, /CARGO_PROFILE_RELEASE_DEBUG=1/)
})
