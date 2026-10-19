package io.virtualtabletop.warpzone;

import android.app.Application;
import android.content.SharedPreferences;
import android.content.pm.PackageInfo;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Process-wide state: settings, the worker pool and the crash reporter.
 *
 * Faults go where the desktop launcher's go: to the server's feedback API
 * (`POST /api/v1/feedback`, game "warpzone", kind "crash" or "bug"), which
 * keeps them and passes them on to whoever looks after Warpzone. A crash is
 * written to a file first and sent on the next start, so a dying process
 * never waits for the network.
 */
public class WzApp extends Application {
    public static final String DEFAULT_SERVER = "https://warpzone.virtualtabletop.io";

    static WzApp app;
    static final Handler main = new Handler(Looper.getMainLooper());
    static final ExecutorService pool = Executors.newFixedThreadPool(4);

    SharedPreferences prefs;
    String version = "?";
    long versionCode;

    @Override
    public void onCreate() {
        super.onCreate();
        app = this;
        prefs = getSharedPreferences("warpzone", MODE_PRIVATE);
        try {
            PackageInfo pi = getPackageManager().getPackageInfo(getPackageName(), 0);
            version = pi.versionName;
            versionCode = pi.getLongVersionCode();
        } catch (Exception ignored) {
        }
        final Thread.UncaughtExceptionHandler previous = Thread.getDefaultUncaughtExceptionHandler();
        Thread.setDefaultUncaughtExceptionHandler((t, e) -> {
            try {
                writeCrash(t, e);
            } catch (Throwable ignored) {
            }
            if (previous != null) previous.uncaughtException(t, e);
            else System.exit(2);
        });
        sendPendingCrash();
    }

    static String server() {
        String s = app.prefs.getString("server", DEFAULT_SERVER).trim();
        while (s.endsWith("/")) s = s.substring(0, s.length() - 1);
        return s.isEmpty() ? DEFAULT_SERVER : s;
    }

    static boolean reportCrashes() {
        return app.prefs.getBoolean("report_crashes", true);
    }

    /** "warpzone-android 0.1.6 (Android 14, API 34, Google Pixel 7)": no personal data. */
    static String client() {
        return "warpzone-android " + app.version + " (Android " + Build.VERSION.RELEASE + ", API "
                + Build.VERSION.SDK_INT + ", " + Build.MANUFACTURER + " " + Build.MODEL + ")";
    }

    static void runOnMain(Runnable r) {
        main.post(r);
    }

    private File crashFile() {
        return new File(getFilesDir(), "crash.txt");
    }

    private void writeCrash(Thread t, Throwable e) throws Exception {
        StringWriter sw = new StringWriter();
        e.printStackTrace(new PrintWriter(sw));
        String title = "Android launcher crash: " + headline(e);
        JSONObject o = new JSONObject();
        o.put("title", title);
        o.put("detail", "thread " + t.getName() + "\n" + sw);
        try (FileOutputStream out = new FileOutputStream(crashFile())) {
            out.write(o.toString().getBytes(StandardCharsets.UTF_8));
        }
    }

    /** Exception class and the first frame of our own code: stable across devices, so repeats are counted. */
    static String headline(Throwable e) {
        Throwable root = e;
        while (root.getCause() != null && root.getCause() != root) root = root.getCause();
        String where = "";
        for (StackTraceElement f : root.getStackTrace()) {
            if (f.getClassName().startsWith("io.virtualtabletop.warpzone")) {
                where = " at " + f.getClassName().replace("io.virtualtabletop.warpzone.", "") + "." + f.getMethodName()
                        + ":" + f.getLineNumber();
                break;
            }
        }
        String s = root.getClass().getSimpleName() + where;
        return s.length() > 150 ? s.substring(0, 150) : s;
    }

    private void sendPendingCrash() {
        final File f = crashFile();
        if (!f.exists()) return;
        pool.execute(() -> {
            try {
                JSONObject o = new JSONObject(new String(Util.readAll(f), StandardCharsets.UTF_8));
                if (reportCrashes()) Api.feedback("warpzone", "crash", o.getString("title"), o.getString("detail"));
                f.delete();
            } catch (java.io.IOException offline) {
                // Kept for the next start.
            } catch (Exception broken) {
                f.delete();
            }
        });
    }

    /**
     * A fault that is not the player's or the network's (a bug in the
     * launcher, an answer it cannot read): reported in the background, at
     * most once per title and run.
     */
    static void reportBug(String what, Throwable e) {
        if (!reportCrashes() || e instanceof java.io.IOException) return;
        final String title = ("Android launcher: " + what + ": " + headline(e));
        synchronized (reported) {
            if (!reported.add(title)) return;
        }
        StringWriter sw = new StringWriter();
        e.printStackTrace(new PrintWriter(sw));
        final String detail = sw.toString();
        pool.execute(() -> {
            try {
                Api.feedback("warpzone", "bug", title, detail);
            } catch (Exception ignored) {
            }
        });
    }

    private static final java.util.HashSet<String> reported = new java.util.HashSet<>();
}
