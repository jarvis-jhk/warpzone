package io.virtualtabletop.warpzone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.text.InputType;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

/** The desktop launcher's look (crates/launcher/src/app.rs colours), for touch. */
final class Ui {
    private Ui() {
    }

    static final int BG = 0xff0b0b14, BAR = 0xff0e0e19, PANEL = 0xff171728, LINE = 0xff2a2a44, FIELD = 0xff12121f,
            BUTTON = 0xff1e1e34, ACCENT = 0xff7c5cff, ACCENT2 = 0xff22d3ee, TEXT = 0xffe2e2f4, MUTED = 0xff9a9ab8,
            DIM = 0xff5c5c78, OK = 0xff34d399, WARN = 0xfffbbf24, BAD = 0xfff87171;

    static Context ctx;

    static int dp(float v) {
        return Math.round(TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v,
                ctx.getResources().getDisplayMetrics()));
    }

    static GradientDrawable round(int fill, float radiusDp, int stroke) {
        GradientDrawable g = new GradientDrawable();
        g.setColor(fill);
        g.setCornerRadius(dp(radiusDp));
        if (stroke != 0) g.setStroke(dp(1), stroke);
        return g;
    }

    static Drawable pressable(Drawable d, float radiusDp) {
        return new RippleDrawable(ColorStateList.valueOf(0x40ffffff), d, round(Color.WHITE, radiusDp, 0));
    }

    static TextView text(String s, float sp, int color) {
        TextView t = new TextView(ctx);
        t.setText(s);
        t.setTextSize(sp);
        t.setTextColor(color);
        t.setLineSpacing(0, 1.12f);
        return t;
    }

    static TextView heading(String s) {
        TextView t = text(s, 26, 0xffb8b8cc);
        t.setTypeface(Typeface.create("sans-serif-black", Typeface.NORMAL));
        t.setPadding(0, dp(4), 0, dp(10));
        return t;
    }

    static TextView section(String s) {
        TextView t = text(s, 18, 0xffa8a8c0);
        t.setTypeface(Typeface.create("sans-serif-black", Typeface.NORMAL));
        t.setPadding(0, dp(18), 0, dp(8));
        return t;
    }

    static TextView button(String label, boolean primary, View.OnClickListener l) {
        TextView b = text(label, 16, primary ? Color.WHITE : TEXT);
        b.setGravity(Gravity.CENTER);
        b.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        b.setMinHeight(dp(48));
        b.setMinWidth(dp(64));
        b.setPadding(dp(18), dp(10), dp(18), dp(10));
        b.setBackground(pressable(round(primary ? ACCENT : BUTTON, 10, 0), 10));
        b.setClickable(true);
        b.setFocusable(true);
        b.setOnClickListener(l);
        return b;
    }

    static void enable(TextView b, boolean on) {
        b.setEnabled(on);
        b.setAlpha(on ? 1f : 0.45f);
    }

    static TextView chip(String s, int color) {
        TextView t = text(s, 12, color);
        t.setPadding(dp(9), dp(2), dp(9), dp(3));
        t.setBackground(round(0x00000000, 20, color));
        return t;
    }

    static LinearLayout row() {
        LinearLayout l = new LinearLayout(ctx);
        l.setOrientation(LinearLayout.HORIZONTAL);
        l.setGravity(Gravity.CENTER_VERTICAL);
        return l;
    }

    static LinearLayout column() {
        LinearLayout l = new LinearLayout(ctx);
        l.setOrientation(LinearLayout.VERTICAL);
        return l;
    }

    static LinearLayout card() {
        LinearLayout l = column();
        l.setBackground(round(PANEL, 12, LINE));
        l.setPadding(dp(16), dp(14), dp(16), dp(14));
        return l;
    }

    static EditText field(String hint, boolean multiLine) {
        EditText e = new EditText(ctx);
        e.setHint(hint);
        e.setHintTextColor(DIM);
        e.setTextColor(TEXT);
        e.setTextSize(16);
        e.setBackground(round(FIELD, 8, LINE));
        e.setPadding(dp(12), dp(10), dp(12), dp(10));
        e.setMinHeight(dp(48));
        if (multiLine) {
            e.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                    | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
            e.setMinLines(4);
            e.setGravity(Gravity.TOP | Gravity.START);
        } else {
            e.setSingleLine(true);
        }
        return e;
    }

    static LinearLayout.LayoutParams wrap() {
        return new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT);
    }

    static LinearLayout.LayoutParams fill() {
        return new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
    }

    static LinearLayout.LayoutParams weight(float w) {
        return new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, w);
    }

    /** Adds `v` to `parent` with margins (dp) right and below. */
    static <T extends View> T add(ViewGroup parent, T v, LinearLayout.LayoutParams lp, int right, int bottom) {
        lp.rightMargin = dp(right);
        lp.bottomMargin = dp(bottom);
        parent.addView(v, lp);
        return v;
    }

    static View gap(int dp) {
        View v = new View(ctx);
        v.setLayoutParams(new LinearLayout.LayoutParams(dp(dp), dp(dp)));
        return v;
    }
}
