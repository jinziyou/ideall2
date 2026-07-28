package dev.gpui.mobile;

import android.app.Activity;
import android.content.Intent;

import java.io.File;
import java.util.ArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicReference;

/** JNI bridge expected by gpui-mobile's file_selector package. */
public final class GpuiFilePicker {
    private static final AtomicReference<ArrayList<String>> RESULT = new AtomicReference<>();
    private static final AtomicReference<String> ERROR = new AtomicReference<>();
    private static CountDownLatch latch;
    private static Intent pendingIntent;
    private static boolean materializeContent;

    public static String openFile(Activity activity, String mimeTypes) {
        ArrayList<String> paths = launch(activity, openIntent(mimeTypes, false), true);
        return paths == null || paths.isEmpty() ? null : paths.get(0);
    }

    public static String[] openFiles(Activity activity, String mimeTypes) {
        ArrayList<String> paths = launch(activity, openIntent(mimeTypes, true), true);
        return paths == null ? null : paths.toArray(new String[0]);
    }

    public static String getSavePath(Activity activity, String mimeType, String suggestedName) {
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(mimeType == null || mimeType.isEmpty() ? "*/*" : mimeType);
        if (suggestedName != null && !suggestedName.isEmpty()) {
            intent.putExtra(Intent.EXTRA_TITLE, suggestedName);
        }
        ArrayList<String> paths = launch(activity, intent, false);
        return paths == null || paths.isEmpty() ? null : paths.get(0);
    }

    public static String getDirectoryPath(Activity activity) {
        ArrayList<String> paths = launch(
            activity,
            new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE),
            false
        );
        return paths == null || paths.isEmpty() ? null : paths.get(0);
    }

    private static Intent openIntent(String mimeTypes, boolean multiple) {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
        String value = mimeTypes == null || mimeTypes.isEmpty() ? "*/*" : mimeTypes;
        String[] types = value.split("\\|");
        if (types.length == 1) {
            intent.setType(types[0]);
        } else {
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_MIME_TYPES, types);
        }
        return intent;
    }

    private static synchronized ArrayList<String> launch(
        Activity activity,
        Intent intent,
        boolean shouldMaterialize
    ) {
        clearDirectory(new File(activity.getCacheDir(), "imports"));
        RESULT.set(null);
        ERROR.set(null);
        latch = new CountDownLatch(1);
        pendingIntent = intent;
        materializeContent = shouldMaterialize;
        Intent proxy = new Intent(activity, GpuiPickerActivity.class);
        proxy.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        activity.startActivity(proxy);
        try {
            latch.await();
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new IllegalStateException("file selection was interrupted", error);
        }
        String error = ERROR.getAndSet(null);
        if (error != null) {
            throw new IllegalStateException(error);
        }
        return RESULT.getAndSet(null);
    }

    static Intent takePendingIntent() {
        Intent intent = pendingIntent;
        pendingIntent = null;
        return intent;
    }

    static boolean shouldMaterializeContent() {
        return materializeContent;
    }

    static void complete(ArrayList<String> paths, String error) {
        RESULT.set(paths);
        ERROR.set(error);
        CountDownLatch current = latch;
        latch = null;
        if (current != null) {
            current.countDown();
        }
    }

    private static void clearDirectory(File file) {
        File[] children = file.listFiles();
        if (children != null) {
            for (File child : children) {
                if (child.isDirectory()) {
                    clearDirectory(child);
                }
                // Cache cleanup is best effort. Android may still hold a file.
                child.delete();
            }
        }
        file.delete();
    }

    private GpuiFilePicker() {}
}
