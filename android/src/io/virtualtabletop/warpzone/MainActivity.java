package io.virtualtabletop.warpzone;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.Typeface;
import android.net.Uri;
import android.os.Bundle;
import android.provider.Settings;
import android.text.TextUtils;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * The whole launcher: one activity, four tabs (Library, Rooms, Feedback,
 * Settings) and a game page, built in code. It shows only games with an
 * Android build (`install.assets.android` in the descriptor).
 */
public class MainActivity extends Activity {
    static MainActivity live;

    private enum Tab {
        LIBRARY("Library"), ROOMS("Rooms"), FEEDBACK("Feedback"), SETTINGS("Settings");

        final String label;

        Tab(String l) {
            label = l;
        }
    }

    private final List<Game> games = new ArrayList<>();
    private boolean loaded, loading, online = true;
    private String loadError;
    private Tab tab = Tab.LIBRARY;
    /** The open game page, or null. */
    private Game page;
    private final Map<String, JSONArray> changelogs = new HashMap<>();
    private final Map<String, JSONArray> rooms = new HashMap<>();
    private long roomsAt;
    private boolean roomsLoading;
    private JSONObject launcherRelease;
    private JSONObject launcherAsset;
    private long updateCheckedAt;
    private String launcherBusy;

    private FrameLayout content;
    private LinearLayout tabBar, banner;
    private TextView status;
    private ScrollView scroll;
    private final Map<String, TileView> tiles = new HashMap<>();
    private TextView pageBusy;
    private ProgressBar pageBar;
    // Feedback form, kept across re-renders.
    private String fbGame = "warpzone", fbKind = "bug", fbTitle = "", fbDetail = "", fbStatus;
    private String codeInput = "";
    private final Map<String, Integer> scrollPos = new HashMap<>();
    private String shown = "";

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        live = this;
        Ui.ctx = this;
        LinearLayout root = Ui.column();
        root.setBackgroundColor(Ui.BG);

        LinearLayout top = Ui.row();
        top.setBackgroundColor(Ui.BAR);
        top.setPadding(Ui.dp(18), Ui.dp(12), Ui.dp(14), Ui.dp(12));
        TextView word = Ui.text("WARPZONE", 21, Color.WHITE);
        word.setTypeface(Typeface.create("sans-serif-black", Typeface.NORMAL));
        word.setLetterSpacing(0.04f);
        top.addView(word, Ui.weight(1));
        status = Ui.text("", 13, Ui.OK);
        top.addView(status, Ui.wrap());
        root.addView(top, Ui.fill());
        View line = new View(this);
        line.setBackgroundColor(Ui.LINE);
        root.addView(line, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, Ui.dp(1)));

        banner = Ui.row();
        banner.setVisibility(View.GONE);
        banner.setBackgroundColor(0xff241f46);
        banner.setPadding(Ui.dp(16), Ui.dp(8), Ui.dp(12), Ui.dp(8));
        root.addView(banner, Ui.fill());

        content = new FrameLayout(this);
        root.addView(content, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));

        tabBar = Ui.row();
        tabBar.setBackgroundColor(Ui.BAR);
        tabBar.setPadding(Ui.dp(8), Ui.dp(6), Ui.dp(8), Ui.dp(8));
        root.addView(tabBar, Ui.fill());
        setContentView(root);

        readCache();
        loadGames();
        handle(getIntent());
        render();
    }

    @Override
    protected void onDestroy() {
        if (live == this) live = null;
        super.onDestroy();
    }

    @Override
    protected void onResume() {
        super.onResume();
        Ui.ctx = this;
        // Installs and uninstalls happen outside: badges and buttons follow.
        render();
        if (System.currentTimeMillis() - updateCheckedAt > 30 * 60 * 1000L) checkLauncherUpdate(false);
        if (loaded && !loading && System.currentTimeMillis() - roomsAt > 15000) loadGames();
    }

    @Override
    protected void onNewIntent(Intent i) {
        super.onNewIntent(i);
        setIntent(i);
        handle(i);
    }

    @Override
    public void onConfigurationChanged(Configuration c) {
        super.onConfigurationChanged(c);
        shown = "";
        render();
    }

    @Override
    public void onBackPressed() {
        if (page != null) {
            page = null;
            render();
        } else if (tab != Tab.LIBRARY) {
            tab = Tab.LIBRARY;
            render();
        } else {
            super.onBackPressed();
        }
    }

    @Override
    public boolean onKeyDown(int code, KeyEvent e) {
        // Controllers: B is back (as on the Deck), shoulder buttons switch tabs.
        if (code == KeyEvent.KEYCODE_BUTTON_B) {
            onBackPressed();
            return true;
        }
        if (code == KeyEvent.KEYCODE_BUTTON_L1 || code == KeyEvent.KEYCODE_BUTTON_R1) {
            int n = Tab.values().length, d = code == KeyEvent.KEYCODE_BUTTON_R1 ? 1 : n - 1;
            tab = Tab.values()[(tab.ordinal() + d) % n];
            page = null;
            render();
            return true;
        }
        return super.onKeyDown(code, e);
    }

    // ---------------------------------------------------------------- data

    private File cacheFile() {
        return new File(getFilesDir(), "games.json");
    }

    private void readCache() {
        try {
            File f = cacheFile();
            if (f.exists()) setGames(new JSONArray(new String(Util.readAll(f), StandardCharsets.UTF_8)), false);
        } catch (Exception e) {
            cacheFile().delete();
        }
    }

    private void setGames(JSONArray all, boolean fresh) {
        Map<String, Game> old = new HashMap<>();
        for (Game g : games) old.put(g.id, g);
        games.clear();
        for (int i = 0; i < all.length(); i++) {
            JSONObject d = all.optJSONObject(i);
            if (d == null || !Game.onAndroid(d)) continue;
            Game g = new Game(d);
            Game prev = old.get(g.id);
            if (prev != null) {
                g.release = prev.release;
                g.busy = prev.busy;
                g.progress = prev.progress;
            }
            JSONObject rel = d.optJSONObject("_release");
            if (g.release == null && rel != null) g.release = rel;
            games.add(g);
        }
        loaded = fresh || loaded || !games.isEmpty();
    }

    private void loadGames() {
        if (loading) return;
        loading = true;
        WzApp.pool.execute(() -> {
            JSONArray all = null;
            String err = null;
            Map<String, JSONObject> rels = new HashMap<>();
            Map<String, JSONArray> rms = new HashMap<>();
            try {
                all = Api.games();
                for (int i = 0; i < all.length(); i++) {
                    JSONObject d = all.getJSONObject(i);
                    if (!Game.onAndroid(d)) continue;
                    String id = d.getString("id");
                    JSONObject r = Api.release(id);
                    if (r != null) {
                        rels.put(id, r);
                        d.put("_release", r);
                    }
                    try {
                        rms.put(id, Api.rooms(id));
                    } catch (Api.HttpError e) {
                        rms.put(id, new JSONArray());
                    }
                }
                try (FileOutputStream out = new FileOutputStream(cacheFile())) {
                    out.write(all.toString().getBytes(StandardCharsets.UTF_8));
                }
            } catch (Exception e) {
                err = e instanceof java.io.IOException ? "offline" : e.toString();
                if (!(e instanceof java.io.IOException)) WzApp.reportBug("game list", e);
            }
            final JSONArray res = all;
            final String error = err;
            WzApp.runOnMain(() -> {
                loading = false;
                online = error == null;
                loadError = error;
                if (res != null) {
                    setGames(res, true);
                    for (Game g : games) if (rels.containsKey(g.id)) g.release = rels.get(g.id);
                    rooms.clear();
                    rooms.putAll(rms);
                    roomsAt = System.currentTimeMillis();
                }
                loaded = true;
                renderIfIdle();
            });
        });
    }

    private void loadRooms() {
        if (roomsLoading) return;
        roomsLoading = true;
        final List<String> ids = new ArrayList<>();
        for (Game g : games) ids.add(g.id);
        WzApp.pool.execute(() -> {
            Map<String, JSONArray> rms = new HashMap<>();
            boolean ok = true;
            for (String id : ids) {
                try {
                    rms.put(id, Api.rooms(id));
                } catch (Exception e) {
                    ok = false;
                    if (!(e instanceof java.io.IOException)) WzApp.reportBug("rooms", e);
                }
            }
            final boolean fine = ok;
            WzApp.runOnMain(() -> {
                roomsLoading = false;
                online = fine;
                if (fine) {
                    rooms.clear();
                    rooms.putAll(rms);
                    roomsAt = System.currentTimeMillis();
                }
                if (tab == Tab.ROOMS || page != null) renderIfIdle();
            });
        });
    }

    private final Runnable roomTicker = new Runnable() {
        @Override
        public void run() {
            WzApp.main.removeCallbacks(this);
            if (live != MainActivity.this || isFinishing()) return;
            if ((tab == Tab.ROOMS && page == null) || page != null) {
                if (hasWindowFocus()) loadRooms();
                WzApp.main.postDelayed(this, 15000);
            }
        }
    };

    private void loadChangelog(final Game g) {
        if (changelogs.containsKey(g.id)) return;
        changelogs.put(g.id, new JSONArray());
        WzApp.pool.execute(() -> {
            try {
                final JSONArray c = Api.changelog(g.id);
                WzApp.runOnMain(() -> {
                    changelogs.put(g.id, c);
                    if (page == g) render();
                });
            } catch (Exception e) {
                if (!(e instanceof java.io.IOException)) WzApp.reportBug("changelog", e);
                WzApp.runOnMain(() -> changelogs.remove(g.id));
            }
        });
    }

    private void checkLauncherUpdate(final boolean asked) {
        updateCheckedAt = System.currentTimeMillis();
        WzApp.pool.execute(() -> {
            try {
                JSONObject u = Api.launcherUpdates();
                JSONObject latest = u.optJSONObject("latest");
                JSONObject assets = latest != null ? latest.optJSONObject("assets") : null;
                JSONObject a = assets != null ? assets.optJSONObject("android") : null;
                boolean avail = u.optBoolean("update_available") && a != null;
                WzApp.runOnMain(() -> {
                    launcherRelease = avail ? latest : null;
                    launcherAsset = avail ? a : null;
                    showBanner();
                    if (asked) toast(avail ? "Warpzone " + latest.optString("version") + " is available"
                            : "Warpzone " + WzApp.app.version + " is the newest");
                });
            } catch (Exception e) {
                if (!(e instanceof java.io.IOException)) WzApp.reportBug("update check", e);
                if (asked) WzApp.runOnMain(() -> toast("Could not check for updates: " + e.getMessage()));
            }
        });
    }

    static void installed(String pkg) {
        MainActivity a = live;
        if (a == null) return;
        for (Game g : a.games) {
            if (g.pkg == null && pkg.equals(Installer.lastPackage)) {
                g.pkg = pkg;
                WzApp.app.prefs.edit().putString("pkg." + g.id, pkg).apply();
            }
        }
        a.render();
    }

    private void toast(String s) {
        Toast.makeText(this, s, Toast.LENGTH_LONG).show();
    }

    private Game game(String id) {
        for (Game g : games) if (g.id.equals(id)) return g;
        return null;
    }

    // ---------------------------------------------------------------- actions

    /** Invite links: warpzone://join/<room>#<secret>, https://<server>/join/<room>#<secret>. */
    private void handle(Intent i) {
        Uri u = i != null ? i.getData() : null;
        if (u == null || !Intent.ACTION_VIEW.equals(i.getAction())) return;
        i.setData(null);
        List<String> seg = u.getPathSegments();
        String room = null;
        if ("warpzone".equals(u.getScheme()) && "join".equals(u.getHost()) && !seg.isEmpty()) room = seg.get(0);
        else if (seg.size() >= 2 && seg.get(0).equals("join")) room = seg.get(1);
        if (room == null || !room.matches("[A-Za-z0-9]{1,16}")) return;
        openInvite(room, u.getFragment(), u.toString());
    }

    private void openInvite(final String room, final String secret, final String link) {
        toast("Opening the invite…");
        WzApp.pool.execute(() -> {
            try {
                final JSONObject info = Api.room(room, secret);
                WzApp.runOnMain(() -> {
                    Game g = game(info.optString("game"));
                    if (g == null) {
                        toast(info.optString("game") + " has no Android version yet");
                        return;
                    }
                    play(g, g.args("join", link));
                });
            } catch (Exception e) {
                WzApp.runOnMain(() -> toast("Invite: " + e.getMessage()));
            }
        });
    }

    private void joinCode(String input) {
        input = input.trim();
        if (input.contains("/join/")) {
            Intent i = new Intent(Intent.ACTION_VIEW, Uri.parse(input));
            handle(i);
            return;
        }
        final String code = input.replaceAll("[\\s-]", "").toUpperCase();
        if (!code.matches("[A-Z0-9]{6}")) {
            toast("A code has 6 letters and digits");
            return;
        }
        final List<Game> have = new ArrayList<>();
        for (Game g : games) if (g.installed() != null) have.add(g);
        if (have.isEmpty()) {
            toast("Install a game first: the game itself joins with the code");
            return;
        }
        if (have.size() == 1) {
            play(have.get(0), Arrays.asList("--warpzone-code", code));
            return;
        }
        String[] names = new String[have.size()];
        for (int k = 0; k < names.length; k++) names[k] = have.get(k).name;
        new AlertDialog.Builder(this).setTitle("Which game is the code for?")
                .setItems(names, (d, w) -> play(have.get(w), Arrays.asList("--warpzone-code", code))).show();
    }

    private void joinRoom(Game g, JSONObject r) {
        final List<String> args = g.args("join", r.optString("id"));
        if ("code".equals(r.optString("entry"))) {
            final EditText f = Ui.field("Code, e.g. K7QM3X", false);
            new AlertDialog.Builder(this).setTitle(r.optString("name")).setMessage("This room wants its code.")
                    .setView(f).setPositiveButton("Join", (d, w) -> {
                        List<String> a = new ArrayList<>(args);
                        a.add("--warpzone-code");
                        a.add(f.getText().toString().replaceAll("[\\s-]", "").toUpperCase());
                        play(g, a);
                    }).setNegativeButton("Cancel", null).show();
            return;
        }
        play(g, args);
    }

    private void play(Game g, List<String> args) {
        if (g.installed() == null) {
            toast("Install " + g.name + " first");
            page = g;
            render();
            return;
        }
        Intent i = g.startIntent(args);
        if (i == null) {
            toast(g.name + " cannot be started");
            return;
        }
        try {
            startActivity(i);
        } catch (Exception e) {
            toast(g.name + ": " + e.getMessage());
            WzApp.reportBug("start game", e);
        }
    }

    private boolean mayInstall() {
        if (getPackageManager().canRequestPackageInstalls()) return true;
        new AlertDialog.Builder(this).setTitle("Allow installing games")
                .setMessage("Android asks once: let Warpzone install the games it downloads. "
                        + "Every install still asks you to confirm.")
                .setPositiveButton("Open settings", (d, w) -> startActivity(new Intent(
                        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName()))))
                .setNegativeButton("Not now", null).show();
        return false;
    }

    private void install(final Game g) {
        if (g.busy != null || g.asset() == null || !mayInstall()) return;
        g.busy = "Starting…";
        g.progress = -1;
        render();
        Installer.install(this, g.name, g.release, g.asset(), new Installer.Listener() {
            @Override
            public void progress(String what, int pct) {
                g.busy = what;
                g.progress = pct;
                showBusy(g);
            }

            @Override
            public void done(String error) {
                g.busy = null;
                g.progress = -1;
                if (error == null && g.pkg == null) {
                    g.pkg = Installer.lastPackage;
                    WzApp.app.prefs.edit().putString("pkg." + g.id, g.pkg).apply();
                }
                if (error != null) toast(error);
                render();
            }
        });
    }

    private void updateLauncher() {
        if (launcherBusy != null || launcherAsset == null || !mayInstall()) return;
        launcherBusy = "Starting…";
        showBanner();
        Installer.install(this, "Warpzone", launcherRelease, launcherAsset, new Installer.Listener() {
            @Override
            public void progress(String what, int pct) {
                launcherBusy = what + (pct >= 0 ? " " + pct + "%" : "");
                showBanner();
            }

            @Override
            public void done(String error) {
                launcherBusy = null;
                if (error != null) toast(error);
                showBanner();
            }
        });
    }

    private void open(String url) {
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
        } catch (Exception e) {
            toast("No app to open " + url);
        }
    }

    // ---------------------------------------------------------------- rendering

    private void showBanner() {
        banner.removeAllViews();
        if (launcherAsset == null) {
            banner.setVisibility(View.GONE);
            return;
        }
        banner.setVisibility(View.VISIBLE);
        String v = launcherRelease.optString("version");
        banner.addView(Ui.text(launcherBusy != null ? launcherBusy : "Warpzone " + v + " is available", 14,
                Ui.TEXT), Ui.weight(1));
        TextView b = Ui.button("Update", true, x -> updateLauncher());
        Ui.enable(b, launcherBusy == null);
        banner.addView(b, Ui.wrap());
    }

    private void renderTabs() {
        tabBar.removeAllViews();
        for (final Tab t : Tab.values()) {
            boolean on = t == tab && (page == null || t == Tab.LIBRARY);
            TextView b = Ui.text(t.label, 15, on ? Color.WHITE : 0xff8c8caa);
            b.setGravity(Gravity.CENTER);
            b.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
            b.setPadding(0, Ui.dp(11), 0, Ui.dp(11));
            b.setBackground(Ui.pressable(Ui.round(on ? Ui.ACCENT : 0x00000000, 10, 0), 10));
            b.setClickable(true);
            b.setFocusable(true);
            b.setOnClickListener(v -> {
                if (tab == t && page == null) {
                    if (scroll != null) scroll.smoothScrollTo(0, 0);
                    return;
                }
                tab = t;
                page = null;
                render();
            });
            LinearLayout.LayoutParams lp = Ui.weight(1);
            lp.leftMargin = lp.rightMargin = Ui.dp(3);
            tabBar.addView(b, lp);
        }
    }

    void render() {
        if (isFinishing()) return;
        Ui.ctx = this;
        status.setText(online ? "online" : "offline");
        status.setTextColor(online ? Ui.OK : Ui.WARN);
        showBanner();
        renderTabs();
        String key = page != null ? "game:" + page.id : tab.name();
        if (scroll != null) scrollPos.put(shown, scroll.getScrollY());
        boolean same = key.equals(shown);
        shown = key;
        tiles.clear();
        pageBusy = null;
        pageBar = null;

        LinearLayout body = Ui.column();
        int pad = Ui.dp(wide() ? 22 : 14);
        body.setPadding(pad, Ui.dp(14), pad, Ui.dp(24));
        if (page != null) gamePage(body, page);
        else if (tab == Tab.LIBRARY) library(body);
        else if (tab == Tab.ROOMS) roomsPage(body);
        else if (tab == Tab.FEEDBACK) feedbackPage(body);
        else settingsPage(body);

        scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.addView(body, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT));
        content.removeAllViews();
        content.addView(scroll);
        final Integer y = same || page == null ? scrollPos.get(key) : null;
        if (y != null && y > 0) scroll.post(() -> scroll.scrollTo(0, y));
        WzApp.main.removeCallbacks(roomTicker);
        if (page != null || tab == Tab.ROOMS) WzApp.main.postDelayed(roomTicker, 15000);
    }

    /** Re-render for new data, but not under the player's typing (the data shows next time). */
    private void renderIfIdle() {
        if (getCurrentFocus() instanceof EditText) return;
        render();
    }

    private boolean wide() {
        return getResources().getConfiguration().screenWidthDp >= 600;
    }

    private LinearLayout header(LinearLayout body, String title, boolean refresh) {
        LinearLayout h = Ui.row();
        h.addView(Ui.heading(title), Ui.weight(1));
        if (refresh) {
            TextView r = Ui.button(loading || roomsLoading ? "…" : "Refresh", false, v -> {
                loadGames();
                render();
            });
            h.addView(r, Ui.wrap());
        }
        body.addView(h, Ui.fill());
        return h;
    }

    private void library(LinearLayout body) {
        header(body, "Library", true);
        if (games.isEmpty()) {
            String msg = !loaded || loading ? "Loading the games…"
                    : loadError != null ? "Cannot reach " + WzApp.server() + " — check the connection, then Refresh."
                            : "No game has an Android version yet.";
            body.addView(Ui.text(msg, 15, Ui.MUTED), Ui.fill());
            return;
        }
        int width = getResources().getConfiguration().screenWidthDp - (wide() ? 44 : 28);
        int cols = Math.max(2, Math.min(8, (width + 12) / 172));
        // As on the Deck: a whole row of tiles fits on the screen (landscape phones).
        int rowSpace = getResources().getConfiguration().screenHeightDp - 190;
        int maxTile = Math.max(90, rowSpace * 2 / 3);
        while (cols < 10 && (width - 12 * (cols - 1)) / cols > maxTile) cols++;
        LinearLayout row = null;
        for (int i = 0; i < games.size(); i++) {
            if (i % cols == 0) {
                row = Ui.row();
                row.setGravity(Gravity.TOP);
                body.addView(row, Ui.fill());
            }
            final Game g = games.get(i);
            TileView t = new TileView(this, g);
            tiles.put(g.id, t);
            badge(t, g);
            t.setOnClickListener(v -> {
                page = g;
                render();
            });
            Ui.add(row, t, Ui.weight(1), i % cols == cols - 1 ? 0 : 12, 12);
        }
        int rest = games.size() % cols;
        for (int k = 0; rest > 0 && k < cols - rest; k++) {
            Ui.add(row, new View(this), Ui.weight(1), k == cols - rest - 1 ? 0 : 12, 12);
        }
        TextView note = Ui.text("Only games with an Android version are shown. Every game for every platform: "
                + "the desktop launcher and " + WzApp.server().replace("https://", ""), 13, Ui.DIM);
        note.setPadding(0, Ui.dp(10), 0, 0);
        body.addView(note, Ui.fill());
    }

    private void badge(TileView t, Game g) {
        if (g.busy != null) t.setBadge(g.progress >= 0 ? g.progress + "%" : "Installing", Ui.ACCENT2);
        else if (g.installed() == null) t.setBadge(g.asset() != null ? "Not installed" : "Coming soon", Ui.MUTED);
        else if (g.updateAvailable()) t.setBadge("Update", Ui.WARN);
        else t.setBadge(null, 0);
    }

    /** Progress without rebuilding the page (keeps scroll and input). */
    private void showBusy(Game g) {
        TileView t = tiles.get(g.id);
        if (t != null) badge(t, g);
        if (page == g && pageBusy != null) {
            pageBusy.setText(g.busy);
            pageBar.setIndeterminate(g.progress < 0);
            if (g.progress >= 0) pageBar.setProgress(g.progress);
        }
    }

    private void gamePage(LinearLayout body, final Game g) {
        loadChangelog(g);
        TextView back = Ui.button("‹  Library", false, v -> onBackPressed());
        Ui.add(body, back, Ui.wrap(), 0, 14);

        LinearLayout head = Ui.row();
        head.setGravity(Gravity.TOP);
        TileView t = new TileView(this, g);
        t.setClickable(false);
        t.setFocusable(false);
        tiles.put(g.id, t);
        head.addView(t, new LinearLayout.LayoutParams(Ui.dp(wide() ? 170 : 116), ViewGroup.LayoutParams.WRAP_CONTENT));
        LinearLayout info = Ui.column();
        info.setPadding(Ui.dp(16), 0, 0, 0);
        TextView name = Ui.text(g.name, 24, Color.WHITE);
        name.setTypeface(Typeface.create("sans-serif-black", Typeface.NORMAL));
        info.addView(name, Ui.fill());
        if (!g.tagline().isEmpty()) Ui.add(info, Ui.text(g.tagline(), 15, Ui.MUTED), Ui.fill(), 0, 8);
        LinearLayout chips = Ui.row();
        for (String tag : g.tags()) Ui.add(chips, Ui.chip(tag, tag.equals("online") || tag.equals("ranked") ? Ui.OK : Ui.WARN), Ui.wrap(), 6, 0);
        HorizontalScrollView hs = new HorizontalScrollView(this);
        hs.setHorizontalScrollBarEnabled(false);
        hs.addView(chips);
        Ui.add(info, hs, Ui.fill(), 0, 8);
        String counts = (g.localPlayers() > 1 ? g.localPlayers() + " players on one device" : "")
                + (g.onlinePlayers() > 0 ? (g.localPlayers() > 1 ? " · " : "") + "up to " + g.onlinePlayers() + " online" : "");
        if (!counts.isEmpty()) Ui.add(info, Ui.text(counts, 13, Ui.TEXT), Ui.fill(), 0, 6);
        String have = g.installedVersion(), latest = g.latestVersion();
        String ver = have != null ? "Installed " + have + (g.updateAvailable() ? " · " + latest + " available" : "")
                : latest != null ? "Version " + latest + (g.asset() != null ? " · " + Util.mb(g.asset().optLong("size")) : "")
                        : "No release yet";
        info.addView(Ui.text(ver, 13, g.updateAvailable() ? Ui.WARN : Ui.DIM), Ui.fill());
        head.addView(info, Ui.weight(1));
        Ui.add(body, head, Ui.fill(), 0, 16);

        LinearLayout actions = Ui.row();
        boolean inst = g.installed() != null;
        if (g.busy == null) {
            if (inst && g.updateAvailable()) {
                Ui.add(actions, Ui.button("Update to " + latest, true, v -> install(g)), Ui.weight(1), 10, 0);
                Ui.add(actions, Ui.button("Play", false, v -> play(g, null)), Ui.weight(1), g.canHost() ? 10 : 0, 0);
            } else if (inst) {
                Ui.add(actions, Ui.button("Play", true, v -> play(g, null)), Ui.weight(1), g.canHost() ? 10 : 0, 0);
            } else {
                TextView b = Ui.button(g.asset() != null ? "Install" : "No Android build yet", true, v -> install(g));
                Ui.enable(b, g.asset() != null);
                Ui.add(actions, b, Ui.weight(1), 0, 0);
            }
            if (inst && g.canHost())
                Ui.add(actions, Ui.button("Host online", false, v -> play(g, g.args("host", null))), Ui.weight(1), 0, 0);
            body.addView(actions, Ui.fill());
        } else {
            pageBusy = Ui.text(g.busy, 14, Ui.ACCENT2);
            body.addView(pageBusy, Ui.fill());
            pageBar = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
            pageBar.setMax(100);
            pageBar.setIndeterminate(g.progress < 0);
            if (g.progress >= 0) pageBar.setProgress(g.progress);
            pageBar.setProgressTintList(android.content.res.ColorStateList.valueOf(Ui.ACCENT));
            body.addView(pageBar, Ui.fill());
        }
        if (inst) {
            TextView un = Ui.text("Uninstall", 13, Ui.DIM);
            un.setPadding(0, Ui.dp(10), 0, 0);
            un.setOnClickListener(v -> {
                try {
                    startActivity(new Intent(Intent.ACTION_DELETE, Uri.parse("package:" + g.pkg)));
                } catch (Exception e) {
                    toast(e.getMessage());
                }
            });
            body.addView(un, Ui.wrap());
        }

        // Open rooms of this game.
        JSONArray rs = rooms.get(g.id);
        if (g.onlinePlayers() > 0) {
            body.addView(Ui.section("Open rooms"), Ui.fill());
            if (rs == null || rs.length() == 0) {
                body.addView(Ui.text(g.canHost() ? "No public rooms right now. Host one, or join a private game with its code on the Rooms tab."
                        : "No public rooms right now.", 14, Ui.MUTED), Ui.fill());
            } else {
                for (int i = 0; i < rs.length(); i++) roomCard(body, g, rs.optJSONObject(i), false);
            }
        }

        // The player's own game files.
        JSONArray data = g.data();
        for (int i = 0; i < data.length(); i++) {
            JSONObject d = data.optJSONObject(i);
            if (d == null) continue;
            if (i == 0) body.addView(Ui.section("Your game files"), Ui.fill());
            LinearLayout c = Ui.card();
            c.addView(Ui.text(d.optString("label"), 16, Ui.TEXT), Ui.fill());
            Ui.add(c, Ui.text(d.optString("description"), 13, Ui.MUTED), Ui.fill(), 0, 8);
            Ui.add(c, Ui.text("On Android the game itself asks for them on its first start.", 13, Ui.DIM), Ui.fill(), 0, 8);
            JSONArray buy = d.optJSONArray("buy");
            LinearLayout links = Ui.row();
            for (int k = 0; buy != null && k < buy.length(); k++) {
                final JSONObject l = buy.optJSONObject(k);
                if (l == null) continue;
                Ui.add(links, Ui.button(l.optString("label"), false, v -> open(l.optString("url"))), Ui.wrap(), 8, 0);
            }
            if (links.getChildCount() > 0) {
                HorizontalScrollView s = new HorizontalScrollView(this);
                s.setHorizontalScrollBarEnabled(false);
                s.addView(links);
                c.addView(s, Ui.fill());
            }
            Ui.add(body, c, Ui.fill(), 0, 10);
        }

        String desc = g.d.optString("description", "");
        if (!desc.isEmpty()) {
            body.addView(Ui.section("About"), Ui.fill());
            Ui.add(body, Ui.text(desc, 15, 0xffc8c8dc), Ui.fill(), 0, 8);
        }
        String req = g.d.optString("requires", "");
        if (!req.isEmpty()) Ui.add(body, Ui.text("Needs: " + req, 13, Ui.MUTED), Ui.fill(), 0, 8);
        final String home = g.d.optString("homepage", "");
        if (!home.isEmpty()) Ui.add(body, Ui.button("Homepage", false, v -> open(home)), Ui.wrap(), 0, 8);

        JSONArray log = changelogs.get(g.id);
        if (log != null && log.length() > 0) {
            body.addView(Ui.section("What's new"), Ui.fill());
            for (int i = 0; i < Math.min(5, log.length()); i++) {
                JSONObject e = log.optJSONObject(i);
                if (e == null) continue;
                LinearLayout c = Ui.card();
                LinearLayout top = Ui.row();
                String title = e.optString("name");
                if (title.isEmpty()) title = e.optString("version");
                top.addView(Ui.text(title, 15, Ui.TEXT), Ui.weight(1));
                top.addView(Ui.text(e.optString("published").split("T")[0], 12, Ui.DIM), Ui.wrap());
                c.addView(top, Ui.fill());
                String notes = Util.plain(e.optString("notes"));
                if (notes.length() > 700) notes = notes.substring(0, 700) + "…";
                if (!notes.isEmpty()) {
                    TextView n = Ui.text(notes, 13, Ui.MUTED);
                    n.setPadding(0, Ui.dp(6), 0, 0);
                    c.addView(n, Ui.fill());
                }
                Ui.add(body, c, Ui.fill(), 0, 10);
            }
        }
    }

    private void roomCard(LinearLayout body, final Game g, final JSONObject r, boolean withGame) {
        if (r == null) return;
        LinearLayout c = Ui.card();
        LinearLayout top = Ui.row();
        LinearLayout left = Ui.column();
        TextView name = Ui.text(r.optString("name"), 16, Color.WHITE);
        name.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        left.addView(name, Ui.fill());
        List<String> bits = new ArrayList<>();
        if (withGame) bits.add(g.name);
        bits.add(r.optInt("players") + " / " + r.optInt("max_players") + " players");
        if (r.optBoolean("ranked")) bits.add("ranked");
        JSONObject aud = r.optJSONObject("audience");
        if (aud != null) {
            JSONArray langs = aud.optJSONArray("languages");
            List<String> ls = new ArrayList<>();
            for (int i = 0; langs != null && i < langs.length(); i++) ls.add(langs.optString(i).toUpperCase());
            if (!ls.isEmpty()) bits.add(TextUtils.join(" ", ls));
            String skill = aud.optString("skill", "any");
            if (!skill.equals("any")) bits.add(skill);
            String style = aud.optString("style", "any");
            if (!style.equals("any")) bits.add(style);
        }
        String entry = r.optString("entry");
        if (entry.equals("code")) bits.add("code needed");
        else if (entry.equals("approve")) bits.add("host lets you in");
        left.addView(Ui.text(TextUtils.join(" · ", bits), 13, Ui.MUTED), Ui.fill());
        // The fields the descriptor wants shown (rooms.display).
        JSONObject meta = r.optJSONObject("metadata");
        JSONObject rd = g.d.optJSONObject("rooms");
        JSONArray disp = rd != null ? rd.optJSONArray("display") : null;
        List<String> shownMeta = new ArrayList<>();
        for (int i = 0; meta != null && disp != null && i < disp.length(); i++) {
            JSONObject f = disp.optJSONObject(i);
            if (f == null) continue;
            String v = meta.optString(f.optString("key"), "");
            if (!v.isEmpty()) shownMeta.add(f.optString("label") + ": " + v);
        }
        if (!shownMeta.isEmpty()) left.addView(Ui.text(TextUtils.join(" · ", shownMeta), 13, Ui.DIM), Ui.fill());
        top.addView(left, Ui.weight(1));
        boolean full = r.optInt("players") >= r.optInt("max_players") && r.optInt("max_players") > 0;
        boolean inst = g.installed() != null;
        TextView b = Ui.button(full ? "Full" : inst ? "Join" : "Install", inst && !full, v -> {
            if (g.installed() == null) {
                page = g;
                render();
            } else {
                joinRoom(g, r);
            }
        });
        Ui.enable(b, !full);
        top.addView(b, Ui.wrap());
        c.addView(top, Ui.fill());
        Ui.add(body, c, Ui.fill(), 0, 10);
    }

    private void roomsPage(LinearLayout body) {
        header(body, "Open rooms", true);
        LinearLayout c = Ui.card();
        c.addView(Ui.text("Join by code or invite link", 14, Ui.MUTED), Ui.fill());
        LinearLayout r = Ui.row();
        r.setPadding(0, Ui.dp(8), 0, 0);
        final EditText code = Ui.field("K7QM3X", false);
        code.setText(codeInput);
        code.setImeOptions(EditorInfo.IME_ACTION_GO);
        code.setInputType(android.text.InputType.TYPE_CLASS_TEXT | android.text.InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS
                | android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        code.addTextChangedListener(new Watch(s -> codeInput = s));
        code.setOnEditorActionListener((v, id, e) -> {
            joinCode(code.getText().toString());
            return true;
        });
        Ui.add(r, code, Ui.weight(1), 10, 0);
        r.addView(Ui.button("Join", true, v -> joinCode(code.getText().toString())), Ui.wrap());
        c.addView(r, Ui.fill());
        Ui.add(body, c, Ui.fill(), 0, 14);

        int n = 0;
        for (Game g : games) {
            JSONArray rs = rooms.get(g.id);
            for (int i = 0; rs != null && i < rs.length(); i++, n++) roomCard(body, g, rs.optJSONObject(i), true);
        }
        if (n == 0) {
            String msg = !online ? "Offline: rooms show up once the server is reachable."
                    : games.isEmpty() ? "No game has an Android version yet."
                            : "No public rooms right now. Host one from a game's page, or join a private game with its code.";
            body.addView(Ui.text(msg, 15, Ui.MUTED), Ui.fill());
        }
    }

    private void feedbackPage(LinearLayout body) {
        header(body, "Feedback", false);
        body.addView(Ui.text("Found a problem or have an idea? No account needed: it goes to the Warpzone server, "
                + "which passes it on to the people who make the game. Please leave out anything personal.", 14, Ui.MUTED), Ui.fill());

        body.addView(Ui.section("About"), Ui.fill());
        LinearLayout which = Ui.row();
        List<String[]> opts = new ArrayList<>();
        opts.add(new String[] {"warpzone", "Warpzone"});
        for (Game g : games) opts.add(new String[] {g.id, g.name});
        for (final String[] o : opts) {
            TextView b = Ui.button(o[1], o[0].equals(fbGame), v -> {
                fbGame = o[0];
                render();
            });
            Ui.add(which, b, Ui.wrap(), 8, 0);
        }
        HorizontalScrollView hs = new HorizontalScrollView(this);
        hs.setHorizontalScrollBarEnabled(false);
        hs.addView(which);
        body.addView(hs, Ui.fill());

        body.addView(Ui.section("What is it"), Ui.fill());
        LinearLayout kinds = Ui.row();
        Ui.add(kinds, Ui.button("Problem", fbKind.equals("bug"), v -> {
            fbKind = "bug";
            render();
        }), Ui.weight(1), 8, 0);
        Ui.add(kinds, Ui.button("Idea", fbKind.equals("idea"), v -> {
            fbKind = "idea";
            render();
        }), Ui.weight(1), 0, 0);
        Ui.add(body, kinds, Ui.fill(), 0, 14);

        final EditText title = Ui.field(fbKind.equals("bug") ? "In one line: what went wrong?" : "In one line: what would you like?", false);
        title.setText(fbTitle);
        title.addTextChangedListener(new Watch(s -> fbTitle = s));
        Ui.add(body, title, Ui.fill(), 0, 10);
        final EditText detail = Ui.field(fbKind.equals("bug") ? "What did you do, what happened, what did you expect? (optional)"
                : "Anything else? (optional)", true);
        detail.setText(fbDetail);
        detail.addTextChangedListener(new Watch(s -> fbDetail = s));
        Ui.add(body, detail, Ui.fill(), 0, 12);
        final TextView send = Ui.button("Send", true, null);
        send.setOnClickListener(v -> {
            final String t = fbTitle.trim();
            if (t.isEmpty()) {
                toast("Write one line first");
                return;
            }
            Ui.enable(send, false);
            fbStatus = "Sending…";
            render();
            final String game = fbGame, kind = fbKind, d = fbDetail.trim();
            WzApp.pool.execute(() -> {
                String res;
                try {
                    Api.feedback(game, kind, t, d);
                    res = "Thank you — it was sent.";
                } catch (Exception e) {
                    res = "Not sent: " + e.getMessage();
                }
                final String r = res;
                WzApp.runOnMain(() -> {
                    fbStatus = r;
                    if (r.startsWith("Thank")) {
                        fbTitle = "";
                        fbDetail = "";
                    }
                    render();
                });
            });
        });
        Ui.add(body, send, Ui.wrap(), 0, 10);
        if (fbStatus != null) body.addView(Ui.text(fbStatus, 14, fbStatus.startsWith("Thank") ? Ui.OK : Ui.MUTED), Ui.fill());
    }

    private void settingsPage(LinearLayout body) {
        header(body, "Settings", false);

        body.addView(Ui.section("Server"), Ui.fill());
        LinearLayout r = Ui.row();
        final EditText srv = Ui.field(WzApp.DEFAULT_SERVER, false);
        srv.setText(WzApp.server());
        srv.setInputType(android.text.InputType.TYPE_CLASS_TEXT | android.text.InputType.TYPE_TEXT_VARIATION_URI);
        Ui.add(r, srv, Ui.weight(1), 10, 0);
        r.addView(Ui.button("Save", false, v -> {
            String s = srv.getText().toString().trim();
            if (s.isEmpty()) s = WzApp.DEFAULT_SERVER;
            if (!s.startsWith("https://") && !s.matches("http://(10\\.0\\.2\\.2|127\\.0\\.0\\.1|localhost)(:\\d+)?/?")) {
                toast("The server address starts with https://");
                return;
            }
            WzApp.app.prefs.edit().putString("server", s).apply();
            games.clear();
            rooms.clear();
            changelogs.clear();
            loaded = false;
            loadGames();
            toast("Saved");
            render();
        }), Ui.wrap());
        body.addView(r, Ui.fill());

        body.addView(Ui.section("Installing games"), Ui.fill());
        boolean may = getPackageManager().canRequestPackageInstalls();
        body.addView(Ui.text(may ? "Warpzone may install games. Android still asks you for every install."
                : "Android asks once before Warpzone may install the games it downloads.", 14, Ui.MUTED), Ui.fill());
        if (!may) {
            TextView b = Ui.button("Allow", false, v -> startActivity(new Intent(
                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName()))));
            LinearLayout.LayoutParams lp = Ui.wrap();
            lp.topMargin = Ui.dp(8);
            body.addView(b, lp);
        }

        body.addView(Ui.section("Problems"), Ui.fill());
        Switch sw = new Switch(this);
        sw.setText("Send crash reports of this app automatically (what crashed and where; nothing personal)");
        sw.setTextColor(Ui.TEXT);
        sw.setTextSize(14);
        sw.setChecked(WzApp.reportCrashes());
        sw.setOnCheckedChangeListener((v, on) -> WzApp.app.prefs.edit().putBoolean("report_crashes", on).apply());
        body.addView(sw, Ui.fill());

        body.addView(Ui.section("Warpzone"), Ui.fill());
        body.addView(Ui.text("Warpzone for Android " + WzApp.app.version, 15, Ui.TEXT), Ui.fill());
        LinearLayout b = Ui.row();
        b.setPadding(0, Ui.dp(8), 0, 0);
        Ui.add(b, Ui.button("Check for updates", false, v -> checkLauncherUpdate(true)), Ui.wrap(), 8, 0);
        Ui.add(b, Ui.button("Website", false, v -> open(WzApp.server())), Ui.wrap(), 8, 0);
        HorizontalScrollView hs = new HorizontalScrollView(this);
        hs.setHorizontalScrollBarEnabled(false);
        hs.addView(b);
        body.addView(hs, Ui.fill());
        body.addView(Ui.button("Source code and licences", false, v -> open("https://github.com/jarvis-jhk/warpzone")),
                withTop(Ui.wrap(), 8));
        TextView about = Ui.text("Open source (MIT OR Apache-2.0). Tile art from SteamGridDB, by its community.", 13, Ui.DIM);
        about.setPadding(0, Ui.dp(12), 0, 0);
        body.addView(about, Ui.fill());
    }

    private static LinearLayout.LayoutParams withTop(LinearLayout.LayoutParams lp, int dp) {
        lp.topMargin = Ui.dp(dp);
        return lp;
    }

    /** TextWatcher that only wants the new text. */
    private static final class Watch implements android.text.TextWatcher {
        interface On {
            void text(String s);
        }

        private final On on;

        Watch(On on) {
            this.on = on;
        }

        @Override
        public void beforeTextChanged(CharSequence s, int a, int b, int c) {
        }

        @Override
        public void onTextChanged(CharSequence s, int a, int b, int c) {
        }

        @Override
        public void afterTextChanged(android.text.Editable s) {
            on.text(s.toString());
        }
    }
}
