package dev.gpui.mobile;

import android.app.Activity;
import android.content.pm.ApplicationInfo;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

import java.util.HashMap;
import java.util.Map;

/**
 * Android platform-view bridge expected by gpui-mobile.
 *
 * ideall currently embeds only isolated system WebViews, so the bridge keeps
 * the supported surface deliberately small instead of packaging GPUI's camera
 * and video helpers that the product does not expose.
 */
public final class GpuiPlatformView {
    private static final String TAG = "GpuiPlatformView";
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static final Map<Long, View> VIEWS = new HashMap<>();
    private static final Map<Long, FrameLayout> CONTAINERS = new HashMap<>();

    private static FrameLayout rootContainer;

    public static boolean createView(
        Activity activity,
        String viewType,
        long viewId,
        float x,
        float y,
        float width,
        float height,
        String creationParams
    ) {
        if (!"container".equals(viewType) && !"webview".equals(viewType)) {
            Log.e(TAG, "Unsupported platform view type: " + viewType);
            return false;
        }

        Map<String, String> params = parseCreationParams(creationParams);
        MAIN.post(() -> {
            try {
                ensureRootContainer(activity);
                float density = activity.getResources().getDisplayMetrics().density;
                FrameLayout container = new FrameLayout(activity);
                container.setLayoutParams(layoutParams(density, x, y, width, height));

                View view = "webview".equals(viewType)
                    ? createWebView(activity, params)
                    : new FrameLayout(activity);
                container.addView(
                    view,
                    new FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.MATCH_PARENT
                    )
                );
                VIEWS.put(viewId, view);
                CONTAINERS.put(viewId, container);
                rootContainer.addView(container);
            } catch (RuntimeException error) {
                Log.e(TAG, "Unable to create platform view " + viewId, error);
                disposeViewOnMainThread(viewId);
            }
        });
        return true;
    }

    public static void setBounds(
        long viewId,
        float x,
        float y,
        float width,
        float height
    ) {
        MAIN.post(() -> {
            FrameLayout container = CONTAINERS.get(viewId);
            if (container == null) return;
            float density = container.getResources().getDisplayMetrics().density;
            container.setLayoutParams(layoutParams(density, x, y, width, height));
        });
    }

    public static void setVisible(long viewId, boolean visible) {
        MAIN.post(() -> {
            FrameLayout container = CONTAINERS.get(viewId);
            if (container != null) {
                container.setVisibility(visible ? View.VISIBLE : View.GONE);
            }
        });
    }

    public static void setZIndex(long viewId, int zIndex) {
        MAIN.post(() -> {
            FrameLayout container = CONTAINERS.get(viewId);
            if (container != null) container.setZ(zIndex);
        });
    }

    public static void disposeView(long viewId) {
        MAIN.post(() -> disposeViewOnMainThread(viewId));
    }

    public static void pauseAll() {
        MAIN.post(() -> {
            for (FrameLayout container : CONTAINERS.values()) {
                container.setVisibility(View.INVISIBLE);
            }
            for (View view : VIEWS.values()) {
                if (view instanceof WebView) ((WebView) view).onPause();
            }
        });
    }

    public static void resumeAll() {
        MAIN.post(() -> {
            for (FrameLayout container : CONTAINERS.values()) {
                container.setVisibility(View.VISIBLE);
            }
            for (View view : VIEWS.values()) {
                if (view instanceof WebView) ((WebView) view).onResume();
            }
        });
    }

    public static void disposeAll() {
        MAIN.post(() -> {
            for (Long viewId : CONTAINERS.keySet().toArray(new Long[0])) {
                disposeViewOnMainThread(viewId);
            }
            if (rootContainer != null) {
                ViewGroup parent = (ViewGroup) rootContainer.getParent();
                if (parent != null) parent.removeView(rootContainer);
                rootContainer = null;
            }
        });
    }

    private static void ensureRootContainer(Activity activity) {
        if (rootContainer != null) return;
        rootContainer = new FrameLayout(activity);
        rootContainer.setClipChildren(false);
        rootContainer.setLayoutParams(
            new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        );
        activity.addContentView(
            rootContainer,
            new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        );
    }

    private static WebView createWebView(Activity activity, Map<String, String> params) {
        WebView webView = new WebView(activity);
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(
            Boolean.parseBoolean(params.getOrDefault("javascript_enabled", "true"))
        );
        settings.setDomStorageEnabled(
            Boolean.parseBoolean(params.getOrDefault("dom_storage_enabled", "true"))
        );
        boolean zoomEnabled = Boolean.parseBoolean(
            params.getOrDefault("zoom_enabled", "true")
        );
        settings.setBuiltInZoomControls(zoomEnabled);
        settings.setDisplayZoomControls(false);
        settings.setSupportZoom(zoomEnabled);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setSafeBrowsingEnabled(true);
        String userAgent = params.get("user_agent");
        if (userAgent != null && !userAgent.isEmpty()) {
            settings.setUserAgentString(userAgent);
        }
        WebView.setWebContentsDebuggingEnabled(
            (activity.getApplicationInfo().flags & ApplicationInfo.FLAG_DEBUGGABLE) != 0
        );
        webView.setWebViewClient(new WebViewClient());

        String html = params.getOrDefault("html", "");
        String url = params.getOrDefault("url", "");
        if (!html.isEmpty()) {
            webView.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null);
        } else if (!url.isEmpty()) {
            webView.loadUrl(url);
        }
        return webView;
    }

    private static void disposeViewOnMainThread(long viewId) {
        View view = VIEWS.remove(viewId);
        FrameLayout container = CONTAINERS.remove(viewId);
        if (container != null) {
            if (rootContainer != null) rootContainer.removeView(container);
            container.removeAllViews();
        }

        if (view instanceof WebView) {
            WebView webView = (WebView) view;
            webView.stopLoading();
            webView.loadUrl("about:blank");
            webView.clearHistory();
            webView.removeAllViews();
            webView.destroy();
        }
    }

    private static FrameLayout.LayoutParams layoutParams(
        float density,
        float x,
        float y,
        float width,
        float height
    ) {
        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(
            Math.max(0, Math.round(width * density)),
            Math.max(0, Math.round(height * density))
        );
        params.leftMargin = Math.round(x * density);
        params.topMargin = Math.round(y * density);
        return params;
    }

    private static Map<String, String> parseCreationParams(String value) {
        Map<String, String> params = new HashMap<>();
        if (value == null || value.isEmpty()) return params;
        for (String pair : value.split("\\|")) {
            int separator = pair.indexOf('=');
            if (separator > 0) {
                params.put(pair.substring(0, separator), pair.substring(separator + 1));
            }
        }
        return params;
    }

    private GpuiPlatformView() {}
}
