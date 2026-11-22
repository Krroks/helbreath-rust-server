//! Helbreath wire frame, as in `Client/Net/XSocket.cpp` (`iSendMsg` / `pGetRcvDataPointer`):
//! `[key u8][size u16 LE, counts the 3 header bytes][body]`, body ciphered when key != 0.

pub const HEADER: usize = 3;

pub fn encrypt(body: &mut [u8], key: u8) {
    if key == 0 {
        return;
    }
    let n = body.len();
    for (i, b) in body.iter_mut().enumerate() {
        *b = b.wrapping_add(i as u8 ^ key);
        *b ^= key ^ (n - i) as u8;
    }
}

pub fn decrypt(body: &mut [u8], key: u8) {
    if key == 0 {
        return;
    }
    let n = body.len();
    for (i, b) in body.iter_mut().enumerate() {
        *b ^= key ^ (n - i) as u8;
        *b = b.wrapping_sub(i as u8 ^ key);
    }
}

/// Builds a full frame from a plain body. Panics if the body does not fit the u16 size field.
pub fn encode(key: u8, body: &[u8]) -> Vec<u8> {
    let size = u16::try_from(body.len() + HEADER).expect("body too large for a frame");
    let mut f = Vec::with_capacity(size as usize);
    f.push(key);
    f.extend_from_slice(&size.to_le_bytes());
    f.extend_from_slice(body);
    encrypt(&mut f[HEADER..], key);
    f
}

/// Splits a full frame into its key and plain body.
pub fn decode(frame: &[u8]) -> (u8, Vec<u8>) {
    let mut body = frame[HEADER..].to_vec();
    decrypt(&mut body, frame[0]);
    (frame[0], body)
}

#[derive(Debug, PartialEq)]
pub struct BadSize(pub u16);

/// Cuts a byte stream (TCP) into whole frames.
#[derive(Default)]
pub struct Splitter {
    buf: Vec<u8>,
}

impl Splitter {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Next complete raw frame, header included. A size below the header can never resync, so it is an error.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, BadSize> {
        if self.buf.len() < HEADER {
            return Ok(None);
        }
        let size = u16::from_le_bytes([self.buf[1], self.buf[2]]);
        if (size as usize) < HEADER {
            return Err(BadSize(size));
        }
        if self.buf.len() < size as usize {
            return Ok(None);
        }
        Ok(Some(self.buf.drain(..size as usize).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // key 1, body [0]: 0 + (0^1) = 1, then 1 ^ (1 ^ 1) = 1
        assert_eq!(encode(1, &[0]), vec![1, 4, 0, 1]);
        assert_eq!(encode(0, &[7, 8]), vec![0, 5, 0, 7, 8]);
    }

    #[test]
    fn roundtrip_all_keys() {
        let body: Vec<u8> = (0..300u32).map(|i| (i * 37 % 256) as u8).collect();
        for key in 0..=255u8 {
            let (k, b) = decode(&encode(key, &body));
            assert_eq!((k, &b), (key, &body));
        }
    }

    #[test]
    fn splitter_partial_and_multiple() {
        let a = encode(9, b"hello");
        let b = encode(0, b"x");
        let mut stream = a.clone();
        stream.extend_from_slice(&b);
        let mut s = Splitter::default();
        s.push(&stream[..2]);
        assert_eq!(s.next_frame(), Ok(None));
        s.push(&stream[2..7]);
        assert_eq!(s.next_frame(), Ok(None));
        s.push(&stream[7..]);
        assert_eq!(s.next_frame(), Ok(Some(a)));
        assert_eq!(s.next_frame(), Ok(Some(b)));
        assert_eq!(s.next_frame(), Ok(None));
        s.push(&[0, 2, 0]);
        assert_eq!(s.next_frame(), Err(BadSize(2)));
    }
}
