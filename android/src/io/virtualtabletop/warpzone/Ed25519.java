package io.virtualtabletop.warpzone;

import java.math.BigInteger;
import java.security.MessageDigest;
import java.util.Arrays;

/**
 * Ed25519 signing (RFC 8032), just what logging in needs: the public key and
 * a signature from a 32-byte seed. Android has no Ed25519 before API 33 and
 * the launcher uses no libraries, so this is the RFC's reference arithmetic
 * with BigInteger: slow by crypto standards (a signature takes milliseconds),
 * which is plenty for a login now and then. Not constant-time; it signs only
 * login challenges, on the player's own device.
 */
final class Ed25519 {
    private Ed25519() {
    }

    private static final BigInteger P = BigInteger.ONE.shiftLeft(255).subtract(BigInteger.valueOf(19));
    private static final BigInteger L = BigInteger.ONE.shiftLeft(252).add(new BigInteger("27742317777372353535851937790883648493"));
    private static final BigInteger D = BigInteger.valueOf(-121665).multiply(BigInteger.valueOf(121666).modInverse(P)).mod(P);
    private static final BigInteger TWO = BigInteger.valueOf(2);
    // The base point, in extended coordinates (X, Y, Z, T).
    private static final BigInteger[] B;

    static {
        BigInteger y = BigInteger.valueOf(4).multiply(BigInteger.valueOf(5).modInverse(P)).mod(P);
        BigInteger x = recoverX(y, false);
        B = new BigInteger[] {x, y, BigInteger.ONE, x.multiply(y).mod(P)};
    }

    private static BigInteger recoverX(BigInteger y, boolean odd) {
        BigInteger y2 = y.multiply(y);
        BigInteger x2 = y2.subtract(BigInteger.ONE).multiply(D.multiply(y2).add(BigInteger.ONE).modInverse(P)).mod(P);
        BigInteger x = x2.modPow(P.add(BigInteger.valueOf(3)).shiftRight(3), P);
        if (!x.multiply(x).subtract(x2).mod(P).equals(BigInteger.ZERO)) {
            x = x.multiply(TWO.modPow(P.subtract(BigInteger.ONE).shiftRight(2), P)).mod(P);
        }
        if (x.testBit(0) != odd) x = P.subtract(x);
        return x;
    }

    private static BigInteger[] add(BigInteger[] p, BigInteger[] q) {
        BigInteger a = p[1].subtract(p[0]).multiply(q[1].subtract(q[0])).mod(P);
        BigInteger b = p[1].add(p[0]).multiply(q[1].add(q[0])).mod(P);
        BigInteger c = TWO.multiply(p[3]).multiply(q[3]).multiply(D).mod(P);
        BigInteger d = TWO.multiply(p[2]).multiply(q[2]).mod(P);
        BigInteger e = b.subtract(a), f = d.subtract(c), g = d.add(c), h = b.add(a);
        return new BigInteger[] {e.multiply(f).mod(P), g.multiply(h).mod(P), f.multiply(g).mod(P), e.multiply(h).mod(P)};
    }

    private static BigInteger[] mul(BigInteger s, BigInteger[] p) {
        BigInteger[] q = {BigInteger.ZERO, BigInteger.ONE, BigInteger.ONE, BigInteger.ZERO};
        for (int i = s.bitLength() - 1; i >= 0; i--) {
            q = add(q, q);
            if (s.testBit(i)) q = add(q, p);
        }
        return q;
    }

    private static byte[] encode(BigInteger[] p) {
        BigInteger zi = p[2].modInverse(P);
        BigInteger x = p[0].multiply(zi).mod(P), y = p[1].multiply(zi).mod(P);
        byte[] out = le(y);
        if (x.testBit(0)) out[31] |= (byte) 0x80;
        return out;
    }

    /** 32 bytes, little-endian. */
    private static byte[] le(BigInteger n) {
        byte[] be = n.toByteArray();
        byte[] out = new byte[32];
        for (int i = 0; i < 32 && i < be.length; i++) out[i] = be[be.length - 1 - i];
        return out;
    }

    private static BigInteger fromLe(byte[] b, int off, int len) {
        byte[] be = new byte[len + 1];
        for (int i = 0; i < len; i++) be[len - i] = b[off + i];
        return new BigInteger(be);
    }

    private static byte[] sha512(byte[]... parts) {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-512");
            for (byte[] p : parts) md.update(p);
            return md.digest();
        } catch (Exception e) {
            throw new IllegalStateException(e);
        }
    }

    private static BigInteger secret(byte[] h) {
        byte[] a = Arrays.copyOf(h, 32);
        a[0] &= (byte) 248;
        a[31] &= 127;
        a[31] |= 64;
        return fromLe(a, 0, 32);
    }

    static byte[] publicKey(byte[] seed) {
        return encode(mul(secret(sha512(seed)), B));
    }

    static byte[] sign(byte[] seed, byte[] msg) {
        byte[] h = sha512(seed);
        BigInteger a = secret(h);
        byte[] pub = encode(mul(a, B));
        BigInteger r = fromLe(sha512(Arrays.copyOfRange(h, 32, 64), msg), 0, 64).mod(L);
        byte[] rEnc = encode(mul(r, B));
        BigInteger k = fromLe(sha512(rEnc, pub, msg), 0, 64).mod(L);
        byte[] s = le(r.add(k.multiply(a)).mod(L));
        byte[] sig = Arrays.copyOf(rEnc, 64);
        System.arraycopy(s, 0, sig, 32, 32);
        return sig;
    }
}
