//! SHA-256, implemented here rather than depended on.
//!
//! Better Files needs one digest for one purpose: telling the user whether the
//! file they downloaded is the file they were promised, and telling a copy
//! whether the bytes arrived. That is about a hundred lines of arithmetic
//! straight out of FIPS 180-4. Adding a cryptography crate to the dependency
//! closure of a file manager to get it — with the licence review AGENTS.md
//! requires for every new dependency — would cost more than it saves.
//!
//! It is not a general-purpose cryptography module and does not claim to be
//! constant-time. It hashes file contents; it verifies no signature.

/// The FIPS 180-4 round constants.
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

/// An incremental SHA-256. Incremental because a checksum job reads a file in
/// the same chunks a copy does, and must stop for a cancellation between them.
#[derive(Clone, Debug)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffered: 0,
            length: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered < 64 {
                // The buffer is still short, which means `data` is exhausted.
                // Falling through would overwrite the partial block with an
                // empty remainder and lose everything buffered so far.
                return;
            }
            let block = self.buffer;
            self.compress(&block);
            self.buffered = 0;
        }
        let mut chunks = data.chunks_exact(64);
        for block in &mut chunks {
            let mut fixed = [0u8; 64];
            fixed.copy_from_slice(block);
            self.compress(&fixed);
        }
        let rest = chunks.remainder();
        self.buffer[..rest.len()].copy_from_slice(rest);
        self.buffered = rest.len();
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.update_padding();
        let mut tail = [0u8; 8];
        tail.copy_from_slice(&bits.to_be_bytes());
        // The length replaces the last eight bytes of the final block.
        let start = self.buffered;
        debug_assert_eq!(start, 56);
        self.buffer[start..start + 8].copy_from_slice(&tail);
        let block = self.buffer;
        self.compress(&block);
        let mut out = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn update_padding(&mut self) {
        self.buffer[self.buffered] = 0x80;
        self.buffered += 1;
        if self.buffered > 56 {
            for byte in self.buffer[self.buffered..].iter_mut() {
                *byte = 0;
            }
            let block = self.buffer;
            self.compress(&block);
            self.buffered = 0;
        }
        for byte in self.buffer[self.buffered..56].iter_mut() {
            *byte = 0;
        }
        self.buffered = 56;
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut schedule = [0u32; 64];
        for (index, slot) in schedule.iter_mut().take(16).enumerate() {
            *slot = u32::from_be_bytes([
                block[index * 4],
                block[index * 4 + 1],
                block[index * 4 + 2],
                block[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        let next = [a, b, c, d, e, f, g, h];
        for (slot, value) in self.state.iter_mut().zip(next) {
            *slot = slot.wrapping_add(value);
        }
    }
}

/// Lowercase hexadecimal, the form every published checksum uses.
pub fn to_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(data: &[u8]) -> String {
        let mut digest = Sha256::new();
        digest.update(data);
        to_hex(&digest.finish())
    }

    #[test]
    fn the_published_test_vectors_match() {
        // FIPS 180-4 and the NESSIE vectors.
        assert_eq!(
            hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn a_million_letters_hash_to_the_published_value() {
        let mut digest = Sha256::new();
        for _ in 0..1000 {
            digest.update(&[b'a'; 1000]);
        }
        assert_eq!(
            to_hex(&digest.finish()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn chunking_does_not_change_the_answer() {
        let data: Vec<u8> = (0..4096u32).map(|value| (value % 251) as u8).collect();
        let whole = hash(&data);
        for chunk in [1usize, 7, 63, 64, 65, 1000] {
            let mut digest = Sha256::new();
            for part in data.chunks(chunk) {
                digest.update(part);
            }
            assert_eq!(to_hex(&digest.finish()), whole, "chunk size {chunk}");
        }
    }

    #[test]
    fn a_block_that_lands_exactly_on_the_padding_boundary_is_handled() {
        // 55, 56, and 64 bytes are the three cases the padding branch splits on.
        for length in [55usize, 56, 57, 63, 64, 119, 120] {
            let data = vec![b'z'; length];
            let mut reference = Sha256::new();
            reference.update(&data);
            let once = to_hex(&reference.finish());
            let mut split = Sha256::new();
            split.update(&data[..length / 2]);
            split.update(&data[length / 2..]);
            assert_eq!(to_hex(&split.finish()), once, "length {length}");
        }
    }
}
