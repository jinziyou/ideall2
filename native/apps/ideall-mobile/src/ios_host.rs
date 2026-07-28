//! Callback registry for services implemented by the Objective-C iOS host.
//!
//! The package emits a `cdylib` for Android and a static library for iOS.
//! Registering the iOS host functions at application startup keeps Cargo's
//! intermediate artifacts self-contained while the final iOS application can
//! still call UIKit-owned services.

use std::{ffi::c_char, sync::OnceLock};

pub(crate) type ShowTextInput = unsafe extern "C" fn(
    value: *const c_char,
    selection_start: usize,
    selection_end: usize,
    keyboard_type: i32,
    multiline: bool,
    secure: bool,
    label: *const c_char,
);
pub(crate) type UpdateTextSelection =
    unsafe extern "C" fn(selection_start: usize, selection_end: usize);
pub(crate) type HideTextInput = unsafe extern "C" fn();
pub(crate) type CopySecurityScopedFile =
    unsafe extern "C" fn(source: *const c_char, destination: *const c_char, max_bytes: u64) -> i32;
pub(crate) type PickFiles = unsafe extern "C" fn() -> *mut c_char;
pub(crate) type FreeString = unsafe extern "C" fn(value: *mut c_char);

#[derive(Clone, Copy)]
struct IosHostCallbacks {
    show_text_input: ShowTextInput,
    update_text_selection: UpdateTextSelection,
    hide_text_input: HideTextInput,
    copy_security_scoped_file: CopySecurityScopedFile,
    pick_files: PickFiles,
    free_string: FreeString,
}

static IOS_HOST_CALLBACKS: OnceLock<IosHostCallbacks> = OnceLock::new();

#[unsafe(no_mangle)]
pub(crate) extern "C" fn ideall_ios_register_host_callbacks(
    show_text_input: ShowTextInput,
    update_text_selection: UpdateTextSelection,
    hide_text_input: HideTextInput,
    copy_security_scoped_file: CopySecurityScopedFile,
    pick_files: PickFiles,
    free_string: FreeString,
) {
    let _ = IOS_HOST_CALLBACKS.set(IosHostCallbacks {
        show_text_input,
        update_text_selection,
        hide_text_input,
        copy_security_scoped_file,
        pick_files,
        free_string,
    });
}

pub(crate) fn show_text_input(
    value: *const c_char,
    selection_start: usize,
    selection_end: usize,
    keyboard_type: i32,
    multiline: bool,
    secure: bool,
    label: *const c_char,
) -> bool {
    let Some(callbacks) = IOS_HOST_CALLBACKS.get() else {
        return false;
    };
    // SAFETY: the Objective-C host registered this callback before starting
    // GPUI, and both C strings remain valid for the duration of the call.
    unsafe {
        (callbacks.show_text_input)(
            value,
            selection_start,
            selection_end,
            keyboard_type,
            multiline,
            secure,
            label,
        );
    }
    true
}

pub(crate) fn update_text_selection(selection_start: usize, selection_end: usize) {
    let Some(callbacks) = IOS_HOST_CALLBACKS.get() else {
        return;
    };
    // SAFETY: the callback is registered by the live iOS application host.
    unsafe { (callbacks.update_text_selection)(selection_start, selection_end) };
}

pub(crate) fn hide_text_input() {
    let Some(callbacks) = IOS_HOST_CALLBACKS.get() else {
        return;
    };
    // SAFETY: the callback is registered by the live iOS application host.
    unsafe { (callbacks.hide_text_input)() };
}

pub(crate) fn copy_security_scoped_file(
    source: *const c_char,
    destination: *const c_char,
    max_bytes: u64,
) -> Option<i32> {
    let callbacks = IOS_HOST_CALLBACKS.get()?;
    // SAFETY: callers retain both C strings through this synchronous callback.
    Some(unsafe { (callbacks.copy_security_scoped_file)(source, destination, max_bytes) })
}

pub(crate) fn pick_files() -> Option<*mut c_char> {
    let callbacks = IOS_HOST_CALLBACKS.get()?;
    // SAFETY: the callback is registered by the live iOS application host.
    Some(unsafe { (callbacks.pick_files)() })
}

pub(crate) unsafe fn free_string(value: *mut c_char) {
    let Some(callbacks) = IOS_HOST_CALLBACKS.get() else {
        return;
    };
    // SAFETY: the caller passes the malloc-owned pointer returned by the
    // paired `pick_files` callback exactly once.
    unsafe { (callbacks.free_string)(value) };
}
