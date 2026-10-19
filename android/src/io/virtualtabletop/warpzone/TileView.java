package io.virtualtabletop.warpzone;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.LinearGradient;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.Shader;
import android.graphics.Typeface;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.view.View;

/**
 * A library tile as on the Deck: portrait 2:3 art, a status badge top right,
 * local and online player counts bottom left. Without art: the game's name
 * on its accent colour.
 */
final class TileView extends View {
    final Game game;
    private Bitmap art;
    private String badge;
    private int badgeColor;
    private final Paint p = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final TextPaint tp = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final RectF r = new RectF();
    private final Path clip = new Path();

    TileView(Context c, Game g) {
        super(c);
        game = g;
        setClickable(true);
        setFocusable(true);
        setForeground(Ui.pressable(null, 12));
        setContentDescription(g.name);
        art = Art.cached(g);
        if (art == null) Art.load(g, b -> {
            art = b;
            invalidate();
        });
    }

    void setBadge(String text, int color) {
        badge = text;
        badgeColor = color;
        invalidate();
    }

    @Override
    protected void onMeasure(int w, int h) {
        int width = MeasureSpec.getSize(w);
        setMeasuredDimension(width, width * 3 / 2);
    }

    @Override
    protected void onDraw(Canvas c) {
        int w = getWidth(), h = getHeight();
        float rad = Ui.dp(12);
        r.set(0, 0, w, h);
        clip.reset();
        clip.addRoundRect(r, rad, rad, Path.Direction.CW);
        c.save();
        c.clipPath(clip);
        if (art != null) {
            // Centre-crop.
            float s = Math.max((float) w / art.getWidth(), (float) h / art.getHeight());
            float bw = art.getWidth() * s, bh = art.getHeight() * s;
            RectF dst = new RectF((w - bw) / 2, (h - bh) / 2, (w + bw) / 2, (h + bh) / 2);
            c.drawBitmap(art, null, dst, p);
        } else {
            int a = game.accent();
            p.setShader(new LinearGradient(0, 0, w, h, a, darken(a, 0.25f), Shader.TileMode.CLAMP));
            c.drawRect(r, p);
            p.setShader(null);
            tp.setColor(Color.WHITE);
            tp.setTypeface(Typeface.create("sans-serif-black", Typeface.NORMAL));
            tp.setTextSize(w / 8f);
            tp.setShadowLayer(Ui.dp(3), 0, Ui.dp(1), 0x99000000);
            StaticLayout sl = StaticLayout.Builder.obtain(game.name, 0, game.name.length(), tp, (int) (w * 0.84f))
                    .setAlignment(Layout.Alignment.ALIGN_CENTER).setMaxLines(4).build();
            c.save();
            c.translate(w * 0.08f, h * 0.62f - sl.getHeight() / 2f);
            sl.draw(c);
            c.restore();
            tp.clearShadowLayer();
        }
        // Shade at the bottom for the counts.
        p.setShader(new LinearGradient(0, h * 0.72f, 0, h, 0x00000000, 0xcc000000, Shader.TileMode.CLAMP));
        c.drawRect(0, h * 0.72f, w, h, p);
        p.setShader(null);
        c.restore();

        float x = Ui.dp(10), base = h - Ui.dp(11);
        tp.setTypeface(Typeface.DEFAULT_BOLD);
        tp.setTextSize(Ui.dp(13));
        tp.setColor(0xffeeeef8);
        int local = game.localPlayers(), online = game.onlinePlayers();
        if (local > 1 || online == 0) {
            pad(c, x, base - Ui.dp(5));
            x += Ui.dp(20);
            c.drawText(String.valueOf(local), x, base, tp);
            x += tp.measureText(String.valueOf(local)) + Ui.dp(10);
        }
        if (online > 0) {
            globe(c, x + Ui.dp(7), base - Ui.dp(5));
            x += Ui.dp(18);
            c.drawText(String.valueOf(online), x, base, tp);
        }
        if (badge != null) {
            tp.setTypeface(Typeface.DEFAULT);
            tp.setTextSize(Ui.dp(11.5f));
            float tw = tp.measureText(badge);
            RectF b = new RectF(w - tw - Ui.dp(22), Ui.dp(8), w - Ui.dp(8), Ui.dp(28));
            p.setColor(0xb8101018);
            c.drawRoundRect(b, Ui.dp(6), Ui.dp(6), p);
            tp.setColor(badgeColor);
            c.drawText(badge, b.left + Ui.dp(7), b.bottom - Ui.dp(6), tp);
        }
        if (isFocused()) {
            p.setStyle(Paint.Style.STROKE);
            p.setStrokeWidth(Ui.dp(2.5f));
            p.setColor(Ui.ACCENT);
            r.inset(Ui.dp(1), Ui.dp(1));
            c.drawRoundRect(r, rad, rad, p);
            p.setStyle(Paint.Style.FILL);
        }
    }

    /** A small gamepad: body and two grips. */
    private void pad(Canvas c, float x, float cy) {
        p.setColor(0xffeeeef8);
        float u = Ui.dp(1);
        c.drawRoundRect(new RectF(x, cy - 5 * u, x + 16 * u, cy + 4 * u), 4 * u, 4 * u, p);
        c.drawCircle(x + 3 * u, cy + 4 * u, 3 * u, p);
        c.drawCircle(x + 13 * u, cy + 4 * u, 3 * u, p);
        p.setColor(0xff202030);
        c.drawRect(x + 3 * u, cy - 1.5f * u, x + 7 * u, cy - 0.5f * u, p);
        c.drawRect(x + 4.5f * u, cy - 3 * u, x + 5.5f * u, cy + 1 * u, p);
        c.drawCircle(x + 11.5f * u, cy - 1 * u, 1.2f * u, p);
    }

    /** A small globe: circle, equator, meridian. */
    private void globe(Canvas c, float cx, float cy) {
        float u = Ui.dp(1);
        p.setStyle(Paint.Style.STROKE);
        p.setStrokeWidth(1.6f * u);
        p.setColor(0xffeeeef8);
        c.drawCircle(cx, cy, 6.5f * u, p);
        c.drawOval(new RectF(cx - 3 * u, cy - 6.5f * u, cx + 3 * u, cy + 6.5f * u), p);
        c.drawLine(cx - 6.5f * u, cy, cx + 6.5f * u, cy, p);
        p.setStyle(Paint.Style.FILL);
    }

    static int darken(int c, float f) {
        return Color.rgb((int) (Color.red(c) * f), (int) (Color.green(c) * f), (int) (Color.blue(c) * f));
    }

    @Override
    protected void onFocusChanged(boolean gain, int dir, Rect prev) {
        super.onFocusChanged(gain, dir, prev);
        invalidate();
    }
}
