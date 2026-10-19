package io.virtualtabletop.warpzone;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;
import org.json.JSONTokener;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;

/** The few parts of the Warpzone HTTP API (docs/api.md) the Android launcher needs. */
final class Api {
    private Api() {
    }

    /** An answer the server gave on purpose (4xx/5xx with its message). */
    static class HttpError extends IOException {
        final int status;

        HttpError(int status, String msg) {
            super(msg);
            this.status = status;
        }
    }

    static String userAgent() {
        return "warpzone-android/" + WzApp.app.version + " (+https://github.com/jarvis-jhk/warpzone)";
    }

    static HttpURLConnection open(String url) throws IOException {
        HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
        c.setConnectTimeout(10000);
        c.setReadTimeout(20000);
        c.setRequestProperty("User-Agent", userAgent());
        c.setInstanceFollowRedirects(true);
        return c;
    }

    private static Object read(HttpURLConnection c) throws IOException, JSONException {
        int code = c.getResponseCode();
        InputStream in = code >= 400 ? c.getErrorStream() : c.getInputStream();
        String body = in == null ? "" : new String(Util.readAll(in), StandardCharsets.UTF_8);
        c.disconnect();
        if (code >= 400) {
            String msg = "HTTP " + code;
            try {
                JSONObject e = new JSONObject(body);
                msg = e.optString("message", e.optString("error", msg));
            } catch (JSONException ignored) {
            }
            throw new HttpError(code, msg);
        }
        return new JSONTokener(body).nextValue();
    }

    static Object get(String url) throws IOException, JSONException {
        HttpURLConnection c = open(url);
        c.setRequestProperty("Accept", "application/json");
        return read(c);
    }

    static Object post(String url, JSONObject body) throws IOException, JSONException {
        return post(url, body, null);
    }

    private static Object post(String url, JSONObject body, String token) throws IOException, JSONException {
        HttpURLConnection c = open(url);
        if (token != null) c.setRequestProperty("Authorization", "Bearer " + token);
        c.setRequestMethod("POST");
        c.setDoOutput(true);
        c.setRequestProperty("Content-Type", "application/json");
        c.setRequestProperty("Accept", "application/json");
        try (OutputStream out = c.getOutputStream()) {
            out.write(body.toString().getBytes(StandardCharsets.UTF_8));
        }
        return read(c);
    }

    static String text(String url) throws IOException {
        HttpURLConnection c = open(url);
        int code = c.getResponseCode();
        if (code >= 400) {
            c.disconnect();
            throw new HttpError(code, "HTTP " + code + " for " + url);
        }
        try (InputStream in = c.getInputStream()) {
            return new String(Util.readAll(in), StandardCharsets.UTF_8);
        } finally {
            c.disconnect();
        }
    }

    static String api(String path) {
        return WzApp.server() + "/api/v1" + path;
    }

    static String q(String s) {
        try {
            return URLEncoder.encode(s, "UTF-8");
        } catch (Exception e) {
            return s;
        }
    }

    static JSONArray games() throws IOException, JSONException {
        return (JSONArray) get(api("/games"));
    }

    /** Newest release of a game, or null when it has none yet. */
    static JSONObject release(String game) throws IOException, JSONException {
        try {
            return (JSONObject) get(api("/games/" + q(game) + "/release"));
        } catch (HttpError e) {
            if (e.status == 404) return null;
            throw e;
        }
    }

    static JSONArray changelog(String game) throws IOException, JSONException {
        return (JSONArray) get(api("/games/" + q(game) + "/changelog"));
    }

    static JSONArray rooms(String game) throws IOException, JSONException {
        return (JSONArray) get(api("/rooms?game=" + q(game)));
    }

    static JSONObject room(String id, String secret) throws IOException, JSONException {
        return (JSONObject) get(api("/rooms/" + q(id) + (secret != null ? "?s=" + q(secret) : "")));
    }

    /** `/games/launcher/updates`: the launcher's own newest release. */
    static JSONObject launcherUpdates() throws IOException, JSONException {
        return (JSONObject) get(api("/games/launcher/updates?current=" + q(WzApp.app.version)));
    }

    static void feedback(String game, String kind, String title, String detail) throws IOException, JSONException {
        JSONObject o = new JSONObject();
        o.put("game", game);
        o.put("kind", kind);
        o.put("title", Util.cut(title.replace('\n', ' ').trim(), 150));
        o.put("detail", Util.cut(detail, 15000));
        o.put("version", Util.cut(WzApp.app.version, 40));
        o.put("client", Util.cut(WzApp.client(), 120));
        post(api("/feedback"), o);
    }

    // ------------------------------------------------------------ account

    private static String token;
    private static String tokenServer;
    /** The account of this device's key as of the last login; null = no name yet. */
    static volatile JSONObject account;
    static volatile boolean loggedIn;

    /** Log in with the device key (challenge, signature, token); returns the account or null. */
    static synchronized JSONObject login() throws IOException, JSONException {
        Identity id = Identity.get();
        JSONObject info = (JSONObject) get(api("/info"));
        JSONObject ch = (JSONObject) post(api("/auth/challenge"), new JSONObject().put("key", id.publicHex));
        String nonce = ch.getString("nonce");
        byte[] msg = ("warpzone-login-v1\n" + info.getString("server_key") + "\n" + nonce).getBytes(StandardCharsets.UTF_8);
        JSONObject req = new JSONObject().put("key", id.publicHex).put("nonce", nonce).put("signature", Util.hex(id.sign(msg)));
        JSONObject r = (JSONObject) post(api("/auth/login"), req);
        token = r.getString("token");
        tokenServer = WzApp.server();
        account = r.optJSONObject("account");
        loggedIn = true;
        return account;
    }

    /** POST as the logged-in device; logs in first, and again once if the token has expired. */
    private static Object authed(String path, JSONObject body) throws IOException, JSONException {
        String t;
        synchronized (Api.class) {
            if (token == null || !WzApp.server().equals(tokenServer)) login();
            t = token;
        }
        try {
            return post(api(path), body, t);
        } catch (HttpError e) {
            if (e.status != 401) throw e;
            synchronized (Api.class) {
                login();
                t = token;
            }
            return post(api(path), body, t);
        }
    }

    /** Claim a player name for this device. */
    static JSONObject register(String username) throws IOException, JSONException {
        JSONObject a = (JSONObject) authed("/accounts", new JSONObject().put("username", username));
        account = a;
        return a;
    }

    /** A code another device redeems to play as this account (valid 10 minutes). */
    static String linkCode() throws IOException, JSONException {
        return ((JSONObject) authed("/accounts/me/link-code", new JSONObject())).getString("code");
    }

    /** Join the account of another device with its link code. */
    static JSONObject link(String code) throws IOException, JSONException {
        JSONObject a = (JSONObject) authed("/accounts/link", new JSONObject().put("code", code.trim()));
        account = a;
        return a;
    }

    static void serverChanged() {
        synchronized (Api.class) {
            token = null;
        }
        account = null;
        loggedIn = false;
    }

    interface Progress {
        /** Return false to cancel. */
        boolean update(long done, long total);
    }

    static void download(String url, File to, long expected, Progress p) throws IOException {
        HttpURLConnection c = open(url);
        c.setReadTimeout(60000);
        int code = c.getResponseCode();
        if (code >= 400) {
            c.disconnect();
            throw new HttpError(code, "download failed: HTTP " + code);
        }
        long total = c.getContentLengthLong() > 0 ? c.getContentLengthLong() : expected;
        File part = new File(to.getPath() + ".part");
        long done = 0;
        try (InputStream in = c.getInputStream(); OutputStream out = new FileOutputStream(part)) {
            byte[] buf = new byte[65536];
            int n;
            long lastReport = 0;
            while ((n = in.read(buf)) > 0) {
                out.write(buf, 0, n);
                done += n;
                if (done - lastReport > 256 * 1024 || done == total) {
                    lastReport = done;
                    if (!p.update(done, total)) throw new IOException("cancelled");
                }
            }
        } finally {
            c.disconnect();
        }
        if (expected > 0 && done != expected) {
            part.delete();
            throw new IOException("download incomplete (" + done + " of " + expected + " bytes)");
        }
        if (!part.renameTo(to)) throw new IOException("could not save " + to.getName());
    }
}
