package io.virtualtabletop.warpzone;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.security.MessageDigest;

final class Util {
    private Util() {
    }

    static byte[] readAll(InputStream in) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] buf = new byte[16384];
        int n;
        while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
        return out.toByteArray();
    }

    static byte[] readAll(File f) throws IOException {
        try (InputStream in = new FileInputStream(f)) {
            return readAll(in);
        }
    }

    static String sha256(File f) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        try (InputStream in = new FileInputStream(f)) {
            byte[] buf = new byte[65536];
            int n;
            while ((n = in.read(buf)) > 0) md.update(buf, 0, n);
        }
        return hex(md.digest());
    }

    static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder();
        for (byte x : b) sb.append(String.format("%02x", x));
        return sb.toString();
    }

    /** Hex to bytes, or null when it is not hex. */
    static byte[] unhex(String s) {
        s = s.trim();
        if (s.length() % 2 != 0) return null;
        byte[] b = new byte[s.length() / 2];
        for (int i = 0; i < b.length; i++) {
            int hi = Character.digit(s.charAt(2 * i), 16), lo = Character.digit(s.charAt(2 * i + 1), 16);
            if (hi < 0 || lo < 0) return null;
            b[i] = (byte) (hi * 16 + lo);
        }
        return b;
    }

    /** The hash a `sha256sum` file lists for `name`, or null. */
    static String checksumFor(String sums, String name) {
        for (String line : sums.split("\n")) {
            String[] p = line.trim().split("\\s+");
            if (p.length == 2 && (p[1].equals(name) || p[1].equals("*" + name))) return p[0].toLowerCase();
        }
        return null;
    }

    /**
     * Whether version `a` is newer than `b` ("0.1.10" > "0.1.9"; a leading
     * "v" and anything after "-" or "+" are ignored for the numbers, and a
     * prerelease is older than the same plain version).
     */
    static boolean newer(String a, String b) {
        if (a == null || b == null) return false;
        int[] x = nums(a), y = nums(b);
        for (int i = 0; i < Math.max(x.length, y.length); i++) {
            int p = i < x.length ? x[i] : 0, q = i < y.length ? y[i] : 0;
            if (p != q) return p > q;
        }
        boolean preA = core(a).length() < strip(a).length(), preB = core(b).length() < strip(b).length();
        return preB && !preA;
    }

    private static String strip(String v) {
        v = v.trim();
        return v.startsWith("v") ? v.substring(1) : v;
    }

    private static String core(String v) {
        v = strip(v);
        int cut = v.length();
        for (char c : new char[] {'-', '+'}) {
            int i = v.indexOf(c);
            if (i >= 0 && i < cut) cut = i;
        }
        return v.substring(0, cut);
    }

    private static int[] nums(String v) {
        String[] p = core(v).split("\\.");
        int[] out = new int[p.length];
        for (int i = 0; i < p.length; i++) {
            try {
                out[i] = Integer.parseInt(p[i].replaceAll("[^0-9].*$", ""));
            } catch (NumberFormatException e) {
                out[i] = 0;
            }
        }
        return out;
    }

    static String mb(long bytes) {
        return bytes >= 1024 * 1024 ? String.format("%.1f MB", bytes / 1048576.0) : (bytes / 1024) + " KB";
    }

    static String cut(String s, int max) {
        if (s == null) return "";
        return s.length() <= max ? s : s.substring(0, max);
    }

    /** Markdown release notes as plain text: no #, *, links shown by their label. */
    static String plain(String md) {
        if (md == null) return "";
        StringBuilder sb = new StringBuilder();
        for (String line : md.split("\n")) {
            String l = line.replaceAll("^#+\\s*", "").replaceAll("^\\s*[*-]\\s+", "• ")
                    .replaceAll("\\[([^\\]]*)\\]\\([^)]*\\)", "$1").replace("**", "").replace("`", "");
            sb.append(l).append('\n');
        }
        return sb.toString().trim();
    }
}
