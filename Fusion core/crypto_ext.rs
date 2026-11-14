//! Hashing, HMAC and operating-system randomness, implemented with the standard library only.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    let add = [a, b, c, d, e, f, g, h];
    for i in 0..8 {
        state[i] = state[i].wrapping_add(add[i]);
    }
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in padded.chunks(64) {
        compress(&mut state, block);
    }
    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        key_block[..32].copy_from_slice(&sha256(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + message.len());
    let mut outer = Vec::with_capacity(96);
    for b in key_block.iter() {
        inner.push(b ^ 0x36);
        outer.push(b ^ 0x5c);
    }
    inner.extend_from_slice(message);
    outer.extend_from_slice(&sha256(&inner));
    sha256(&outer)
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn from_hex(text: &str) -> Result<Vec<u8>, String> {
    let t = text.trim();
    if t.len() % 2 != 0 || !t.is_ascii() {
        return Err("hex string must have an even number of ASCII digits".to_string());
    }
    (0..t.len() / 2)
        .map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).map_err(|_| "invalid hex digit".to_string()))
        .collect()
}

/// Compares two byte strings without early exit on the first difference.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(windows)]
#[link(name = "advapi32")]
extern "system" {
    // RtlGenRandom, exported by advapi32 as SystemFunction036.
    fn SystemFunction036(buffer: *mut u8, length: u32) -> u8;
}

#[cfg(windows)]
fn fill_secure(buf: &mut [u8]) -> Result<(), String> {
    let mut done = 0usize;
    while done < buf.len() {
        let chunk = std::cmp::min(buf.len() - done, u32::MAX as usize) as u32;
        // SAFETY: the pointer is valid for `chunk` bytes inside `buf`.
        let ok = unsafe { SystemFunction036(buf.as_mut_ptr().add(done), chunk) };
        if ok == 0 {
            return Err("operating system random generator failed".to_string());
        }
        done += chunk as usize;
    }
    Ok(())
}

#[cfg(unix)]
fn fill_secure(buf: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    let mut file = std::fs::File::open("/dev/urandom").map_err(|e| format!("open /dev/urandom: {}", e))?;
    file.read_exact(buf).map_err(|e| format!("read /dev/urandom: {}", e))
}

#[cfg(not(any(windows, unix)))]
fn fill_secure(_buf: &mut [u8]) -> Result<(), String> {
    Err("secure randomness is not available on this platform".to_string())
}

/// Returns `n` cryptographically secure random bytes from the operating system.
pub fn secure_random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    if n > 0 {
        fill_secure(&mut buf)?;
    }
    Ok(buf)
}

pub fn secure_random_u64() -> Result<u64, String> {
    let b = secure_random_bytes(8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(&b);
    Ok(u64::from_le_bytes(a))
}

/// Uniform integer in `lo..=hi` using rejection sampling over secure random bytes.
pub fn secure_random_range(lo: i64, hi: i64) -> Result<i64, String> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let span = (hi as i128 - lo as i128 + 1) as u128;
    if span == 1 {
        return Ok(lo);
    }
    let limit = (u64::MAX as u128 + 1) / span * span;
    loop {
        let r = secure_random_u64()? as u128;
        if r < limit {
            return Ok((lo as i128 + (r % span) as i128) as i64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(to_hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(to_hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(to_hex(&sha256(long.as_bytes())), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    }

    #[test]
    fn hmac_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(to_hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn round_constants_match_their_definition() {
        // K[i] is the first 32 fractional bits of the cube root of the i-th prime.
        let mut primes = Vec::new();
        let mut n = 2u32;
        while primes.len() < 64 {
            if (2..n).take_while(|d| d * d <= n).all(|d| n % d != 0) {
                primes.push(n);
            }
            n += 1;
        }
        for (i, p) in primes.iter().enumerate() {
            let root = (*p as f64).cbrt();
            let frac = root - root.floor();
            assert_eq!((frac * 4294967296.0) as u32, K[i], "constant {}", i);
        }
    }

    #[test]
    fn hex_and_compare() {
        assert_eq!(from_hex("00ff10").unwrap(), vec![0, 255, 16]);
        assert!(from_hex("abc").is_err());
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[test]
    fn secure_random_works() {
        let a = secure_random_bytes(16).unwrap();
        let b = secure_random_bytes(16).unwrap();
        assert_eq!(a.len(), 16);
        assert_ne!(a, b);
        let r = secure_random_range(5, 9).unwrap();
        assert!((5..=9).contains(&r));
    }
}
