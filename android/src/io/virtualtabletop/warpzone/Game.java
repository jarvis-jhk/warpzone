package io.virtualtabletop.warpzone;

import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.graphics.Color;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.List;

/** A game descriptor (docs/descriptors.md) plus what this device knows about it. */
final class Game {
    final JSONObject d;
    final String id, name;
    /** Package of the game's APK (`launch.exec.android`); may be learnt from the downloaded APK. */
    String pkg;
    JSONObject release;
    /** Set while a download or install runs: what the button shows instead. */
    String busy;
    int progress = -1;

    Game(JSONObject d) {
        this.d = d;
        id = d.optString("id");
        name = d.optString("name", id);
        JSONObject exec = launch().optJSONObject("exec");
        pkg = exec != null ? exec.optString("android", null) : null;
        if (pkg == null || pkg.isEmpty()) pkg = WzApp.app.prefs.getString("pkg." + id, null);
    }

    /** Only games with an Android build show up on Android. */
    static boolean onAndroid(JSONObject d) {
        JSONObject inst = d.optJSONObject("install");
        JSONObject assets = inst != null ? inst.optJSONObject("assets") : null;
        return !d.optBoolean("hidden") && assets != null && assets.has("android");
    }

    JSONObject launch() {
        JSONObject l = d.optJSONObject("launch");
        return l != null ? l : new JSONObject();
    }

    String tagline() {
        return d.optString("tagline", "");
    }

    int accent() {
        try {
            return Color.parseColor(d.optString("accent", "#7c5cff"));
        } catch (Exception e) {
            return 0xff7c5cff;
        }
    }

    int localPlayers() {
        return d.optInt("local_players", 1);
    }

    int onlinePlayers() {
        JSONObject r = d.optJSONObject("rooms");
        return r != null ? r.optInt("max_players", 0) : 0;
    }

    long steamGridDb() {
        return d.optLong("steamgriddb", 0);
    }

    boolean canHost() {
        JSONArray h = launch().optJSONArray("host");
        return h != null && h.length() > 0 && d.optInt("tier") >= 2;
    }

    JSONObject asset() {
        JSONObject a = release != null ? release.optJSONObject("assets") : null;
        return a != null ? a.optJSONObject("android") : null;
    }

    String latestVersion() {
        return release != null ? release.optString("version", null) : null;
    }

    PackageInfo installed() {
        if (pkg == null) return null;
        try {
            return WzApp.app.getPackageManager().getPackageInfo(pkg, 0);
        } catch (PackageManager.NameNotFoundException e) {
            return null;
        }
    }

    String installedVersion() {
        PackageInfo p = installed();
        return p != null ? p.versionName : null;
    }

    boolean updateAvailable() {
        String have = installedVersion(), latest = latestVersion();
        return have != null && asset() != null && Util.newer(latest, have);
    }

    /** The game's own start intent with the launcher's arguments (`warpzone.args`, see crates/core/src/launch.rs). */
    Intent startIntent(List<String> args) {
        if (pkg == null) return null;
        Intent i = WzApp.app.getPackageManager().getLaunchIntentForPackage(pkg);
        if (i == null) return null;
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        if (args != null && !args.isEmpty()) {
            i.putExtra("warpzone.args", args.toArray(new String[0]));
            // A running game gets the new intent instead of being restarted.
            i.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP);
        }
        i.putExtra("warpzone.launched", true);
        i.putExtra("warpzone.server", WzApp.server());
        // The player's key, as desktop games share identity.key: once it has
        // a name, every game plays as that player (launch::adopt_identity).
        if (Api.account != null) i.putExtra("warpzone.identity", Identity.get().seedHex());
        return i;
    }

    /** `launch.join` / `launch.host` with `{room}` filled in. */
    List<String> args(String key, String room) {
        List<String> out = new ArrayList<>();
        JSONArray a = launch().optJSONArray(key);
        if (a == null) return out;
        for (int i = 0; i < a.length(); i++) out.add(a.optString(i).replace("{room}", room == null ? "" : room));
        return out;
    }

    /** The `[[data]]` entries: game files the player brings. */
    JSONArray data() {
        JSONArray a = d.optJSONArray("data");
        return a != null ? a : new JSONArray();
    }

    List<String> tags() {
        List<String> t = new ArrayList<>();
        String status = d.optString("status", "");
        if (!status.isEmpty()) t.add(status);
        if (d.optInt("tier") >= 1) t.add("online");
        JSONArray ladder = d.optJSONArray("ladder");
        if (ladder != null && ladder.length() > 0) t.add("ranked");
        return t;
    }
}
