//! Native iOS/Android text-input and accessibility proxy bridge.

use gpui_mobile::KeyboardType;

#[derive(Clone, Debug)]
pub(crate) struct NativeTextState {
    pub(crate) value: String,
    pub(crate) selection_start: usize,
    pub(crate) selection_end: usize,
    pub(crate) composing: bool,
}

static PENDING_NATIVE_TEXT: std::sync::Mutex<Option<NativeTextState>> = std::sync::Mutex::new(None);

#[cfg(any(target_os = "ios", target_os = "android"))]
fn enqueue_native_text_state(state: NativeTextState) {
    if let Ok(mut pending) = PENDING_NATIVE_TEXT.lock() {
        *pending = Some(state);
        gpui_mobile::TEXT_INPUT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
    }
}

pub(crate) fn take_pending_text() -> Option<NativeTextState> {
    PENDING_NATIVE_TEXT
        .lock()
        .ok()
        .and_then(|mut pending| pending.take())
}

pub(crate) fn clear_pending_text() {
    if let Ok(mut pending) = PENDING_NATIVE_TEXT.lock() {
        pending.take();
    }
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ideall_mobile_native_text_state(
    value: *const std::ffi::c_char,
    selection_start: usize,
    selection_end: usize,
    composing: bool,
) {
    if value.is_null() {
        return;
    }
    // SAFETY: UIKit supplies a valid, NUL-terminated UTF-8 pointer for the
    // duration of this callback and the text is copied before returning.
    let value = unsafe { std::ffi::CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    enqueue_native_text_state(NativeTextState {
        value,
        selection_start,
        selection_end,
        composing,
    });
}

#[cfg(target_os = "android")]
pub(crate) unsafe fn android_on_text_input(
    _env: *mut std::ffi::c_void,
    _activity: *mut std::ffi::c_void,
    value: *mut std::ffi::c_void,
    selection_start: i32,
    selection_end: i32,
    composing: u8,
) {
    let value_raw = value as jni::sys::jobject;
    let _ = gpui_mobile::android::jni::with_env(|env| {
        let value = unsafe { jni::objects::JString::from_raw(env, value_raw) };
        enqueue_native_text_state(NativeTextState {
            value: value.to_string(),
            selection_start: selection_start.max(0) as usize,
            selection_end: selection_end.max(0) as usize,
            composing: composing != 0,
        });
        Ok(())
    });
}

#[cfg(target_os = "android")]
pub(crate) fn set_android_safe_area_insets(left: i32, top: i32, right: i32, bottom: i32) {
    gpui_mobile::android::jni::update_safe_area_insets(left, top, right, bottom);
}

#[cfg(any(target_os = "ios", target_os = "android"))]
fn keyboard_type_code(keyboard_type: KeyboardType) -> i32 {
    match keyboard_type {
        KeyboardType::Default => 0,
        KeyboardType::EmailAddress => 1,
        KeyboardType::Phone => 2,
        KeyboardType::NumberPad => 3,
        KeyboardType::URL => 4,
        KeyboardType::Decimal => 5,
    }
}

pub(crate) fn show(
    value: &str,
    selection_start: usize,
    selection_end: usize,
    keyboard_type: KeyboardType,
    multiline: bool,
    secure: bool,
    label: &str,
) -> bool {
    #[cfg(target_os = "ios")]
    {
        let value = std::ffi::CString::new(value.replace('\0', "\u{fffd}"))
            .expect("replacement text cannot contain NUL");
        let label = std::ffi::CString::new(label).expect("field label cannot contain NUL");
        crate::ios_host::show_text_input(
            value.as_ptr(),
            selection_start,
            selection_end,
            keyboard_type_code(keyboard_type),
            multiline,
            secure,
            label.as_ptr(),
        )
    }
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;

        gpui_mobile::android::jni::with_env(|env| {
            let activity = gpui_mobile::android::jni::activity(env)?;
            let value = env.new_string(value).map_err(|error| error.to_string())?;
            let label = env.new_string(label).map_err(|error| error.to_string())?;
            env.call_method(
                &activity,
                jni::jni_str!("showIdeallTextInput"),
                jni::jni_sig!("(Ljava/lang/String;IIIZZLjava/lang/String;)V"),
                &[
                    JValue::Object(&value),
                    JValue::Int(selection_start.min(i32::MAX as usize) as i32),
                    JValue::Int(selection_end.min(i32::MAX as usize) as i32),
                    JValue::Int(keyboard_type_code(keyboard_type)),
                    JValue::Bool(multiline.into()),
                    JValue::Bool(secure.into()),
                    JValue::Object(&label),
                ],
            )
            .map_err(|error| {
                env.exception_describe();
                env.exception_clear();
                error.to_string()
            })?;
            Ok(())
        })
        .is_ok()
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        let _ = (
            value,
            selection_start,
            selection_end,
            keyboard_type,
            multiline,
            secure,
            label,
        );
        false
    }
}

pub(crate) fn update_selection(selection_start: usize, selection_end: usize) {
    #[cfg(target_os = "ios")]
    crate::ios_host::update_text_selection(selection_start, selection_end);
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;

        let _ = gpui_mobile::android::jni::with_env(|env| {
            let activity = gpui_mobile::android::jni::activity(env)?;
            env.call_method(
                &activity,
                jni::jni_str!("updateIdeallTextSelection"),
                jni::jni_sig!("(II)V"),
                &[
                    JValue::Int(selection_start.min(i32::MAX as usize) as i32),
                    JValue::Int(selection_end.min(i32::MAX as usize) as i32),
                ],
            )
            .map_err(|error| {
                env.exception_describe();
                env.exception_clear();
                error.to_string()
            })?;
            Ok(())
        });
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    let _ = (selection_start, selection_end);
}

pub(crate) fn hide() {
    #[cfg(target_os = "ios")]
    crate::ios_host::hide_text_input();
    #[cfg(target_os = "android")]
    {
        let _ = gpui_mobile::android::jni::with_env(|env| {
            let activity = gpui_mobile::android::jni::activity(env)?;
            env.call_method(
                &activity,
                jni::jni_str!("hideIdeallTextInput"),
                jni::jni_sig!("()V"),
                &[],
            )
            .map_err(|error| {
                env.exception_describe();
                env.exception_clear();
                error.to_string()
            })?;
            Ok(())
        });
    }
}
