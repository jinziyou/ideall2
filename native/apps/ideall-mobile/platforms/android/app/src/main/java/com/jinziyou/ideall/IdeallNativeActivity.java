package com.jinziyou.ideall;

import android.app.NativeActivity;
import android.graphics.Color;
import android.graphics.Insets;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.FrameLayout;
import java.nio.charset.StandardCharsets;

/**
 * NativeActivity host with a tiny real EditText proxy.
 *
 * GPUI still paints the editor. The proxy supplies Android's full InputConnection,
 * composing spans, selection semantics, password handling, and TalkBack metadata.
 */
public final class IdeallNativeActivity extends NativeActivity {
    private BridgeEditText textInputBridge;
    private boolean updatingTextInputBridge;

    private native void nativeOnTextInput(
        String value,
        int selectionStart,
        int selectionEnd,
        boolean composing
    );

    private native void nativeSetSafeAreaInsets(int left, int top, int right, int bottom);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
    }

    public void onIdeallNativeReady() {
        runOnUiThread(() -> {
            installWindowInsetsBridge();
            installTextInputBridgeIfNeeded();
        });
    }

    private void installWindowInsetsBridge() {
        View decorView = getWindow().getDecorView();
        decorView.setOnApplyWindowInsetsListener((view, windowInsets) -> {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                Insets insets = windowInsets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout()
                );
                nativeSetSafeAreaInsets(insets.left, insets.top, insets.right, insets.bottom);
            } else {
                nativeSetSafeAreaInsets(
                    windowInsets.getSystemWindowInsetLeft(),
                    windowInsets.getSystemWindowInsetTop(),
                    windowInsets.getSystemWindowInsetRight(),
                    windowInsets.getSystemWindowInsetBottom()
                );
            }
            return windowInsets;
        });
        decorView.post(decorView::requestApplyInsets);
    }

    private void installTextInputBridgeIfNeeded() {
        if (textInputBridge != null) return;

        BridgeEditText input = new BridgeEditText();
        input.setBackgroundColor(Color.TRANSPARENT);
        input.setTextColor(Color.TRANSPARENT);
        input.setHintTextColor(Color.TRANSPARENT);
        input.setCursorVisible(false);
        input.setAlpha(0.02f);
        input.setPadding(0, 0, 0, 0);
        input.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_YES);
        input.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        input.setContentDescription("文本输入");
        input.setHint("编辑后内容会自动保存在本机");
        input.setVisibility(View.GONE);
        input.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence value, int start, int count, int after) {}

            @Override
            public void onTextChanged(CharSequence value, int start, int before, int count) {}

            @Override
            public void afterTextChanged(Editable value) {
                dispatchTextInputState();
            }
        });

        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(2, 2);
        params.gravity = Gravity.TOP | Gravity.START;
        addContentView(input, params);
        textInputBridge = input;
    }

    public void showIdeallTextInput(
        String value,
        int selectionStart,
        int selectionEnd,
        int keyboardType,
        boolean multiline,
        boolean secure,
        String label
    ) {
        runOnUiThread(() -> {
            installTextInputBridgeIfNeeded();
            BridgeEditText input = textInputBridge;
            if (input == null) return;

            updatingTextInputBridge = true;
            input.setContentDescription(label == null || label.isEmpty() ? "文本输入" : label);
            input.setSingleLine(!multiline);
            input.setMaxLines(multiline ? Integer.MAX_VALUE : 1);
            input.setInputType(androidInputType(keyboardType, multiline, secure));
            input.setImeOptions(multiline
                ? EditorInfo.IME_FLAG_NO_ENTER_ACTION
                : EditorInfo.IME_ACTION_DONE);
            input.setText(value == null ? "" : value);
            int start = byteOffsetToUtf16(input.getText().toString(), selectionStart);
            int end = byteOffsetToUtf16(input.getText().toString(), selectionEnd);
            start = Math.max(0, Math.min(start, input.length()));
            end = Math.max(start, Math.min(end, input.length()));
            input.setSelection(start, end);
            input.setVisibility(View.VISIBLE);
            updatingTextInputBridge = false;
            input.requestFocus();
            InputMethodManager manager = getSystemService(InputMethodManager.class);
            if (manager != null) {
                manager.restartInput(input);
                manager.showSoftInput(input, InputMethodManager.SHOW_IMPLICIT);
            }
            input.sendAccessibilityEvent(android.view.accessibility.AccessibilityEvent.TYPE_VIEW_FOCUSED);
        });
    }

    public void updateIdeallTextSelection(int selectionStart, int selectionEnd) {
        runOnUiThread(() -> {
            BridgeEditText input = textInputBridge;
            if (input == null) return;
            updatingTextInputBridge = true;
            String value = input.getText().toString();
            int start = byteOffsetToUtf16(value, selectionStart);
            int end = byteOffsetToUtf16(value, selectionEnd);
            start = Math.max(0, Math.min(start, input.length()));
            end = Math.max(start, Math.min(end, input.length()));
            input.setSelection(start, end);
            updatingTextInputBridge = false;
        });
    }

    public void hideIdeallTextInput() {
        runOnUiThread(() -> {
            BridgeEditText input = textInputBridge;
            if (input == null) return;
            InputMethodManager manager = getSystemService(InputMethodManager.class);
            if (manager != null) {
                manager.hideSoftInputFromWindow(input.getWindowToken(), 0);
            }
            input.clearFocus();
            input.setVisibility(View.GONE);
        });
    }

    private int androidInputType(int keyboardType, boolean multiline, boolean secure) {
        if (secure) {
            return InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD;
        }
        int type;
        switch (keyboardType) {
            case 1:
                type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS;
                break;
            case 2:
                type = InputType.TYPE_CLASS_PHONE;
                break;
            case 3:
                type = InputType.TYPE_CLASS_NUMBER;
                break;
            case 4:
                type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI;
                break;
            case 5:
                type = InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_FLAG_DECIMAL;
                break;
            default:
                type = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES;
                break;
        }
        if (multiline) type |= InputType.TYPE_TEXT_FLAG_MULTI_LINE;
        return type;
    }

    private void dispatchTextInputState() {
        BridgeEditText input = textInputBridge;
        if (updatingTextInputBridge || input == null) return;
        Editable value = input.getText();
        int start = Math.max(0, input.getSelectionStart());
        int end = Math.max(start, input.getSelectionEnd());
        boolean composing = BaseInputConnection.getComposingSpanStart(value) >= 0;
        nativeOnTextInput(
            value.toString(),
            utf16OffsetToBytes(value.toString(), start),
            utf16OffsetToBytes(value.toString(), end),
            composing
        );
    }

    private static int utf16OffsetToBytes(String value, int offset) {
        offset = Math.max(0, Math.min(offset, value.length()));
        return value.substring(0, offset).getBytes(StandardCharsets.UTF_8).length;
    }

    private static int byteOffsetToUtf16(String value, int byteOffset) {
        int target = Math.max(0, byteOffset);
        int utf16 = 0;
        int bytes = 0;
        while (utf16 < value.length()) {
            int codePoint = value.codePointAt(utf16);
            int width = new String(Character.toChars(codePoint))
                .getBytes(StandardCharsets.UTF_8)
                .length;
            if (bytes + width > target) break;
            bytes += width;
            utf16 += Character.charCount(codePoint);
        }
        return utf16;
    }

    private final class BridgeEditText extends EditText {
        BridgeEditText() {
            super(IdeallNativeActivity.this);
        }

        @Override
        protected void onSelectionChanged(int selectionStart, int selectionEnd) {
            super.onSelectionChanged(selectionStart, selectionEnd);
            dispatchTextInputState();
        }
    }
}
