package io.virtualtabletop.warpzone;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;

/**
 * This device's Warpzone identity: one Ed25519 key, the login (no password,
 * no e-mail), kept as a hex seed in the app's private files like the
 * desktop's identity.key. Games started from here get it (`--warpzone-identity`
 * in `warpzone.args`) once it has a player name, so on Android, too, every
 * game plays as the same player.
 */
final class Identity {
    final byte[] seed;
    final String publicHex;

    private Identity(byte[] seed) {
        this.seed = seed;
        this.publicHex = Util.hex(Ed25519.publicKey(seed));
    }

    private static Identity cached;

    static synchronized Identity get() {
        if (cached != null) return cached;
        File f = new File(WzApp.app.getFilesDir(), "identity.key");
        byte[] seed = null;
        try {
            if (f.exists()) seed = Util.unhex(new String(Util.readAll(f), StandardCharsets.UTF_8));
        } catch (Exception ignored) {
        }
        if (seed == null || seed.length != 32) {
            seed = new byte[32];
            new SecureRandom().nextBytes(seed);
            File tmp = new File(f.getPath() + ".tmp");
            try (FileOutputStream out = new FileOutputStream(tmp)) {
                out.write(Util.hex(seed).getBytes(StandardCharsets.UTF_8));
                out.getFD().sync();
            } catch (Exception e) {
                throw new IllegalStateException("cannot save the device key", e);
            }
            if (!tmp.renameTo(f)) throw new IllegalStateException("cannot save the device key");
        }
        cached = new Identity(seed);
        return cached;
    }

    byte[] sign(byte[] msg) {
        return Ed25519.sign(seed, msg);
    }

    String seedHex() {
        return Util.hex(seed);
    }
}
