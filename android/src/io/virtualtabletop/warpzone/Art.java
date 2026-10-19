package io.virtualtabletop.warpzone;

import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.util.LruCache;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.util.HashSet;
import java.util.Set;

/**
 * Library tiles: the best portrait grid of the game on SteamGridDB (as the
 * desktop launcher picks it, crates/core/src/griddb.rs: public endpoints, no
 * key; static, not NSFW/humour/epilepsy), else the server's own art, else a
 * tile drawn from the game's accent colour (TileView). Images are cached in
 * the app's cache directory, so the library also has its art offline.
 */
final class Art {
    private Art() {
    }

    interface Done {
        void art(Bitmap b);
    }

    private static final String[] IMAGE_HOSTS = {"https://cdn2.steamgriddb.com/", "https://cdn.steamgriddb.com/",
            "https://steamgriddb.com/"};
    private static final LruCache<String, Bitmap> mem = new LruCache<String, Bitmap>(24 * 1024 * 1024) {
        @Override
        protected int sizeOf(String k, Bitmap b) {
            return b.getByteCount();
        }
    };
    private static final Set<String> loading = new HashSet<>();

    static Bitmap cached(Game g) {
        return mem.get(g.id);
    }

    static void load(final Game g, final Done done) {
        synchronized (loading) {
            if (mem.get(g.id) != null || !loading.add(g.id)) return;
        }
        WzApp.pool.execute(() -> {
            Bitmap b = null;
            try {
                b = fetch(g);
            } catch (Exception e) {
                if (e instanceof RuntimeException) WzApp.reportBug("tile art", e);
            }
            final Bitmap res = b;
            synchronized (loading) {
                loading.remove(g.id);
            }
            if (res != null) {
                mem.put(g.id, res);
                WzApp.runOnMain(() -> done.art(res));
            }
        });
    }

    private static File dir() {
        File d = new File(WzApp.app.getCacheDir(), "art");
        d.mkdirs();
        return d;
    }

    private static Bitmap decode(File f) {
        BitmapFactory.Options o = new BitmapFactory.Options();
        o.inJustDecodeBounds = true;
        BitmapFactory.decodeFile(f.getPath(), o);
        int sample = 1;
        while (o.outWidth / (sample * 2) >= 300) sample *= 2;
        o = new BitmapFactory.Options();
        o.inSampleSize = sample;
        return BitmapFactory.decodeFile(f.getPath(), o);
    }

    private static Bitmap fetch(Game g) throws Exception {
        File f = new File(dir(), g.id + ".img");
        if (f.exists()) {
            Bitmap b = decode(f);
            if (b != null) {
                // Refresh a week-old choice in the background next time; the old one shows meanwhile.
                if (System.currentTimeMillis() - f.lastModified() < 7L * 86400000L) return b;
                String url = pick(g);
                if (url != null && save(url, f)) {
                    Bitmap n = decode(f);
                    if (n != null) return n;
                }
                f.setLastModified(System.currentTimeMillis());
                return b;
            }
        }
        String url = pick(g);
        if (url != null && save(url, f)) return decode(f);
        return null;
    }

    private static String pick(Game g) {
        long sgdb = g.steamGridDb();
        if (sgdb > 0) {
            try {
                JSONObject body = new JSONObject();
                body.put("asset_type", "grid");
                body.put("game_id", new JSONArray().put(sgdb));
                body.put("order", "score_desc");
                body.put("page", 0);
                body.put("dimensions", new JSONArray().put("600x900").put("342x482").put("660x930"));
                JSONObject a = (JSONObject) Api.post("https://www.steamgriddb.com/api/public/search/assets", body);
                JSONObject data = a.optJSONObject("data");
                JSONArray assets = data != null ? data.optJSONArray("assets") : null;
                for (int i = 0; assets != null && i < assets.length(); i++) {
                    JSONObject x = assets.getJSONObject(i);
                    boolean ok = !x.optBoolean("nsfw") && !x.optBoolean("humor") && !x.optBoolean("epilepsy")
                            && !x.optBoolean("is_animated") && x.optInt("height") > x.optInt("width")
                            && !x.optString("mime").contains("gif");
                    if (ok) return x.optString("url");
                }
            } catch (Exception e) {
                // Offline or the public endpoint changed: the server's art or the drawn tile.
            }
        }
        String art = g.d.optString("art", "");
        if (art.isEmpty() || art.equals("null")) return WzApp.server() + "/art/" + g.id + ".jpg";
        return art.startsWith("http") ? art : WzApp.server() + (art.startsWith("/") ? art : "/static/" + art);
    }

    private static boolean save(String url, File f) {
        boolean ours = url.startsWith(WzApp.server() + "/");
        boolean sgdb = false;
        for (String h : IMAGE_HOSTS) sgdb |= url.startsWith(h);
        if (!ours && !sgdb) return false;
        try {
            HttpURLConnection c = Api.open(url);
            if (c.getResponseCode() != 200) {
                c.disconnect();
                return false;
            }
            File part = new File(f.getPath() + ".part");
            try (InputStream in = c.getInputStream(); FileOutputStream out = new FileOutputStream(part)) {
                byte[] buf = new byte[65536];
                int n;
                long total = 0;
                while ((n = in.read(buf)) > 0) {
                    total += n;
                    if (total > 16L * 1024 * 1024) throw new java.io.IOException("image too large");
                    out.write(buf, 0, n);
                }
            } finally {
                c.disconnect();
            }
            return part.renameTo(f);
        } catch (Exception e) {
            return false;
        }
    }
}
