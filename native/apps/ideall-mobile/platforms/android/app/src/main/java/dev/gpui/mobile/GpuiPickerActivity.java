package dev.gpui.mobile;

import android.app.Activity;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.UUID;

/** Transparent proxy used because NativeActivity does not expose activity results to Rust. */
public final class GpuiPickerActivity extends Activity {
    private static final int REQUEST_PICK = 9202;
    private static final long MAX_IMPORT_BYTES = 256L * 1024L * 1024L;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (state != null) {
            finishWithError("file picker was recreated; please retry the import");
            return;
        }
        Intent pending = GpuiFilePicker.takePendingIntent();
        if (pending == null) {
            finishWithError("file picker request is missing");
            return;
        }
        try {
            startActivityForResult(pending, REQUEST_PICK);
        } catch (RuntimeException error) {
            finishWithError("unable to open the system file picker");
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQUEST_PICK || resultCode != RESULT_OK || data == null) {
            GpuiFilePicker.complete(null, null);
            finish();
            return;
        }
        try {
            ArrayList<Uri> uris = collectUris(data);
            ArrayList<String> results = new ArrayList<>(uris.size());
            for (Uri uri : uris) {
                results.add(
                    GpuiFilePicker.shouldMaterializeContent()
                        ? materialize(uri).getAbsolutePath()
                        : uri.toString()
                );
            }
            GpuiFilePicker.complete(results, null);
        } catch (Exception error) {
            GpuiFilePicker.complete(null, "unable to copy the selected document into ideall");
        }
        finish();
    }

    private ArrayList<Uri> collectUris(Intent data) {
        ArrayList<Uri> uris = new ArrayList<>();
        ClipData clips = data.getClipData();
        if (clips != null) {
            for (int index = 0; index < clips.getItemCount(); index += 1) {
                Uri uri = clips.getItemAt(index).getUri();
                if (uri != null) uris.add(uri);
            }
        } else if (data.getData() != null) {
            uris.add(data.getData());
        }
        return uris;
    }

    private File materialize(Uri uri) throws Exception {
        String displayName = queryDisplayName(uri);
        String safeName = displayName.replaceAll("[^\\p{L}\\p{N}._ -]", "_");
        if (safeName.isEmpty() || safeName.equals(".") || safeName.equals("..")) {
            safeName = "document.bin";
        }
        if (safeName.length() > 180) {
            safeName = safeName.substring(safeName.length() - 180);
        }
        File directory = new File(
            new File(getCacheDir(), "imports"),
            UUID.randomUUID().toString()
        );
        if (!directory.mkdirs()) {
            throw new IllegalStateException("unable to create import cache");
        }
        File destination = new File(directory, safeName);
        try (
            InputStream source = getContentResolver().openInputStream(uri);
            FileOutputStream sink = new FileOutputStream(destination)
        ) {
            if (source == null) throw new IllegalStateException("selected document is unavailable");
            byte[] buffer = new byte[64 * 1024];
            long total = 0;
            int count;
            while ((count = source.read(buffer)) >= 0) {
                total += count;
                if (total > MAX_IMPORT_BYTES) {
                    throw new IllegalStateException("selected document exceeds import budget");
                }
                sink.write(buffer, 0, count);
            }
        } catch (Exception error) {
            destination.delete();
            throw error;
        }
        return destination;
    }

    private String queryDisplayName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    String name = cursor.getString(column);
                    if (name != null && !name.isEmpty()) return name;
                }
            }
        }
        String fallback = uri.getLastPathSegment();
        return fallback == null || fallback.isEmpty() ? "document.bin" : fallback;
    }

    private void finishWithError(String message) {
        GpuiFilePicker.complete(null, message);
        finish();
    }
}
