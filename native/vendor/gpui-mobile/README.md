# gpui-mobile source snapshot

This directory vendors [`itsbalamurali/gpui-mobile`](https://github.com/itsbalamurali/gpui-mobile)
at revision `1d3ec2a1d14a63b74d1f4269340441d4eeada27a`.

The snapshot is kept local because ideall needs a small iOS runtime hardening patch that has not
landed upstream, plus workspace compatibility cleanup:

- serialize FFI frame pumps at the `IosWindow` boundary;
- retain GPUI through UIKit's externally driven run loop with `Application::run_embedded`;
- avoid panicking when momentum or frame callback state is already borrowed;
- release the momentum borrow before dispatching GPUI input;
- expose the native iOS root view so host overlays attach to GPUI's actual input window;
- replace the active Android platform registry after `NativeActivity` recreation instead of
  retaining a dispatcher bound to the previous `android_main` thread;
- bridge Android system-bar and display-cutout `WindowInsets` into GPUI logical safe areas,
  including Android 15 edge-to-edge windows whose NDK content rect remains empty;
- keep vendored code warning-free under ideall's host-target Clippy gate;
- disable standalone doctests for upstream snippets that require application-defined factories;
- let the parent workspace own dependency patches and build profiles;
- pin Zed `gpui` and `gpui_wgpu` to revision
  `74798c68d5c63d31e2ccca5c8f5ec0a02c90679c`, which provides the embedded application handle.
- use `gpui_wgpu`'s wgpu re-export so the mobile binaries contain only the locked crates.io
  wgpu `29.0.4` source.

The machine-readable source is in `upstream.env`; `PATCHED_FILES.sha256` locks every locally
modified file. The upstream Apache, GPL, and AGPL license files are retained unchanged.

Run `bash native/scripts/verify-gpui-mobile-vendor.sh` for an offline integrity check. Add
`--upstream` to fetch the exact revision and prove that no unrecorded source files differ. When
upgrading, replace `src/` and `Cargo.toml`, reapply the documented compatibility changes, update
`upstream.env` and `PATCHED_FILES.sha256`, then run the native host plus Android/iOS smoke tests.
