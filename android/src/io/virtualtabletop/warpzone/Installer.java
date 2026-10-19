package io.virtualtabletop.warpzone;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageInstaller;

import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * Downloads an APK from a release, checks it against the release's
 * SHA256SUMS (fetched from GitHub, not from the Warpzone server) and hands
 * it to Android's package installer, which asks the player.
 */
final class Installer {
    private Installer() {
    }

    interface Listener {
        void progress(String what, int percent);

        /** Called on the main thread; error is null when Android took the APK. */
        void done(String error);
    }

    /** Called by Receiver on the main thread with the installer's verdict. */
    static Listener current;

    static void install(final Context ctx, final String label, final JSONObject release, final JSONObject asset,
            final Listener l) {
        WzApp.pool.execute(() -> {
            try {
                String name = asset.getString("name");
                if (!name.matches("[A-Za-z0-9._+-]+\\.apk")) throw new IOException("unexpected file " + name);
                String sumsUrl = release.optString("checksums", "");
                if (sumsUrl.isEmpty() || sumsUrl.equals("null"))
                    throw new IOException("the release has no SHA256SUMS: not installing an unchecked APK");
                String want = Util.checksumFor(Api.text(sumsUrl), name);
                if (want == null) throw new IOException(name + " is not in the release's SHA256SUMS");
                File dir = new File(ctx.getCacheDir(), "apk");
                dir.mkdirs();
                for (File old : dir.listFiles()) old.delete();
                File apk = new File(dir, name);
                final long size = asset.optLong("size", 0);
                Api.download(asset.getString("url"), apk, size, (done, total) -> {
                    int pct = total > 0 ? (int) (done * 100 / total) : -1;
                    WzApp.runOnMain(() -> l.progress("Downloading " + label + " · " + Util.mb(done), pct));
                    return true;
                });
                WzApp.runOnMain(() -> l.progress("Checking " + label + "…", -1));
                String got = Util.sha256(apk);
                if (!got.equals(want)) {
                    apk.delete();
                    throw new IOException("checksum mismatch for " + name + ": not installed");
                }
                PackageInfo pi = ctx.getPackageManager().getPackageArchiveInfo(apk.getPath(), 0);
                if (pi == null) throw new IOException(name + " is not a valid APK");
                final String pkg = pi.packageName;
                WzApp.runOnMain(() -> {
                    try {
                        commit(ctx, apk, pkg, l);
                    } catch (Exception e) {
                        l.done("Install failed: " + e.getMessage());
                        WzApp.reportBug("install", e);
                    }
                });
            } catch (final Exception e) {
                if (!(e instanceof IOException)) WzApp.reportBug("install", e);
                WzApp.runOnMain(() -> l.done(e.getMessage() != null ? e.getMessage() : e.toString()));
            }
        });
    }

    /** The package an APK installs, remembered so a game without `launch.exec.android` can be started. */
    static String lastPackage;

    private static void commit(Context ctx, File apk, String pkg, Listener l) throws IOException {
        lastPackage = pkg;
        PackageInstaller pi = ctx.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(
                PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(pkg);
        params.setSize(apk.length());
        int id = pi.createSession(params);
        try (PackageInstaller.Session s = pi.openSession(id)) {
            try (InputStream in = new FileInputStream(apk); OutputStream out = s.openWrite("base.apk", 0, apk.length())) {
                byte[] buf = new byte[65536];
                int n;
                while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                s.fsync(out);
            }
            Intent cb = new Intent(ctx, Receiver.class).putExtra("pkg", pkg);
            PendingIntent p = PendingIntent.getBroadcast(ctx, id, cb,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            current = l;
            l.progress("Installing…", -1);
            s.commit(p.getIntentSender());
        }
    }

    /** The package installer's answers: ask the player, then success or failure. */
    public static class Receiver extends BroadcastReceiver {
        @Override
        public void onReceive(Context ctx, Intent i) {
            int status = i.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
            Listener l = current;
            if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                Intent confirm = i.getParcelableExtra(Intent.EXTRA_INTENT);
                if (confirm != null) {
                    confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    ctx.startActivity(confirm);
                }
                return;
            }
            current = null;
            if (l == null) return;
            if (status == PackageInstaller.STATUS_SUCCESS) {
                String pkg = i.getStringExtra("pkg");
                l.done(null);
                if (pkg != null) MainActivity.installed(pkg);
            } else if (status == PackageInstaller.STATUS_FAILURE_ABORTED) {
                l.done("Install cancelled");
            } else {
                String msg = i.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
                l.done("Android did not install it" + (msg != null ? ": " + msg : ""));
            }
        }
    }
}
