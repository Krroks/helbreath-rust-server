//! Byte/string helpers ported from Login/main.cpp, StrTok.cpp and the CRT calls the C++ relies on.
//! Strings are latin1 bytes; `l1`/`to_l1` map them 1:1 to `char` so `format!` can build SQL and logs.

/// C string view: bytes up to the first NUL.
pub fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// `IsSame`: equal C strings.
pub fn is_same(a: &[u8], b: &[u8]) -> bool {
    cstr(a) == cstr(b)
}

pub fn l1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

pub fn to_l1(s: &str) -> Vec<u8> {
    s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
}

/// `MakeGoodName`: backslash before `'`, `\` and `"`.
pub fn make_good_name(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() * 2);
    for &c in cstr(src) {
        if c == b'\'' || c == b'\\' || c == b'"' {
            out.push(b'\\');
        }
        out.push(c);
    }
    out
}

/// Escaped name as a latin1 `String`, ready for `format!`.
pub fn good(src: &[u8]) -> String {
    l1(&make_good_name(src))
}

/// Leading whitespace, sign and digits, as the CRT parsers read them.
fn parse_prefix(s: &[u8]) -> (bool, u128) {
    let s = cstr(s);
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let mut v: u128 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as u128).min(u64::MAX as u128 + 1);
        i += 1;
    }
    (neg, v)
}

/// MSVC `atoi`: clamps to INT_MIN/INT_MAX on overflow.
pub fn atoi(s: &[u8]) -> i32 {
    let (neg, v) = parse_prefix(s);
    let v = if neg { -(v as i128) } else { v as i128 };
    v.clamp(i32::MIN as i128, i32::MAX as i128) as i32
}

/// `strtoul(s, NULL, 10)`: ULONG_MAX on overflow, a leading '-' negates modulo 2^32.
pub fn atoul(s: &[u8]) -> u32 {
    let (neg, v) = parse_prefix(s);
    if v > u32::MAX as u128 {
        return u32::MAX;
    }
    if neg { (v as u32).wrapping_neg() } else { v as u32 }
}

/// `_strtoui64(s, NULL, 10)`.
pub fn atoull(s: &[u8]) -> u64 {
    let (neg, v) = parse_prefix(s);
    if v > u64::MAX as u128 {
        return u64::MAX;
    }
    if neg { (v as u64).wrapping_neg() } else { v as u64 }
}

/// Reader over a received message. The C++ copies the message into a buffer two bytes longer and
/// then reads fixed offsets, sometimes past the end (heap garbage there); we read zeros instead.
#[derive(Clone, Copy)]
pub struct Rd<'a> {
    d: &'a [u8],
    base: usize,
}

impl<'a> Rd<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Rd { d, base: 0 }
    }
    /// Bytes actually received from here on.
    pub fn len(&self) -> usize {
        self.d.len().saturating_sub(self.base)
    }
    pub fn at(&self, off: usize) -> Rd<'a> {
        Rd { d: self.d, base: self.base + off }
    }
    pub fn u8(&self, off: usize) -> u8 {
        self.d.get(self.base + off).copied().unwrap_or(0)
    }
    pub fn u16(&self, off: usize) -> u16 {
        u16::from_le_bytes([self.u8(off), self.u8(off + 1)])
    }
    pub fn u32(&self, off: usize) -> u32 {
        u32::from_le_bytes([self.u8(off), self.u8(off + 1), self.u8(off + 2), self.u8(off + 3)])
    }
    pub fn u64(&self, off: usize) -> u64 {
        self.u32(off) as u64 | (self.u32(off + 4) as u64) << 32
    }
    /// `n` raw bytes (zero padded).
    pub fn raw(&self, off: usize, n: usize) -> Vec<u8> {
        (0..n).map(|i| self.u8(off + i)).collect()
    }
    /// `SafeCopy(buf, cp+off, n)` then used as a C string.
    pub fn s(&self, off: usize, n: usize) -> Vec<u8> {
        cstr(&self.raw(off, n)).to_vec()
    }
}

/// Writer over a fixed, zeroed send buffer. Out-of-range writes are dropped (the C++ would corrupt memory).
pub struct W<'a>(pub &'a mut [u8]);

impl W<'_> {
    pub fn bytes(&mut self, off: usize, b: &[u8]) {
        for (i, &c) in b.iter().enumerate() {
            if let Some(x) = self.0.get_mut(off + i) {
                *x = c;
            }
        }
    }
    pub fn u8(&mut self, off: usize, v: u8) {
        self.bytes(off, &[v]);
    }
    pub fn u16(&mut self, off: usize, v: u16) {
        self.bytes(off, &v.to_le_bytes());
    }
    pub fn u32(&mut self, off: usize, v: u32) {
        self.bytes(off, &v.to_le_bytes());
    }
    pub fn u64(&mut self, off: usize, v: u64) {
        self.bytes(off, &v.to_le_bytes());
    }
    /// `SafeCopy(dst, src)` / `SafeCopy(dst, src, strlen(src))`: the string and a NUL, nothing at all when empty.
    pub fn cstr(&mut self, off: usize, s: &[u8]) {
        let s = cstr(s);
        if s.is_empty() {
            return;
        }
        self.bytes(off, s);
        self.u8(off + s.len(), 0);
    }
}

/// Port of `CStrTok::pGet` (StrTok.cpp), quirks included: a run of separators ends a token,
/// CR is a separator only when followed by LF, a leading separator yields an empty token.
pub struct StrTok<'a> {
    data: &'a [u8],
    seps: &'a [u8],
    cur: usize,
}

impl<'a> StrTok<'a> {
    pub fn new(data: &'a [u8], seps: &'a [u8]) -> Self {
        StrTok { data: cstr(data), seps, cur: 0 }
    }

    fn is_sep(&self, c: u8, next: u8) -> bool {
        c == 0 || (c == 0x0d && next == 0x0a) || self.seps.contains(&c)
    }

    fn next_of(&self, i: usize) -> u8 {
        let len = self.data.len() as isize;
        if (i as isize) <= len - 2 { self.data[i + 1] } else { 0 }
    }

    pub fn get(&mut self) -> Option<Vec<u8>> {
        let len = self.data.len();
        let mut tok = Vec::new();
        while self.cur < len {
            if self.is_sep(self.data[self.cur], self.next_of(self.cur)) {
                loop {
                    let mut done = false;
                    if self.is_sep(self.data[self.cur], self.next_of(self.cur)) {
                        self.cur += 1;
                    } else {
                        done = true;
                    }
                    if self.cur as isize >= len as isize - 1 {
                        done = true;
                    }
                    if done {
                        break;
                    }
                }
                return Some(tok);
            }
            tok.push(self.data[self.cur]);
            self.cur += 1;
        }
        if tok.is_empty() { None } else { Some(tok) }
    }
}

/// `CLoginServer::IsDatePast`: compares "Y-M-D h:m:s" with the local time, field by field; equal counts as past.
pub fn is_date_past(date: &[u8], now: [i32; 6]) -> bool {
    let mut t = StrTok::new(date, b" :-");
    for field in now {
        let v = atoi(&t.get().unwrap_or_default());
        if v < field {
            return true;
        }
        if v > field {
            return false;
        }
    }
    true
}

/// `fopen(.., "rt")` + `fread` + `strlen`: CRLF becomes LF, Ctrl-Z ends the file, the first NUL ends the string.
pub fn text_mode(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            0x1a | 0 => break,
            b'\r' if raw.get(i + 1) == Some(&b'\n') => {}
            c => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_name_escapes() {
        assert_eq!(make_good_name(b"a'b\\c\"d"), b"a\\'b\\\\c\\\"d");
        assert_eq!(make_good_name(b"plain\0junk"), b"plain");
        assert_eq!(make_good_name(b""), b"");
    }

    #[test]
    fn crt_parsers() {
        assert_eq!(atoi(b"  -12x"), -12);
        assert_eq!(atoi(b"99999999999"), i32::MAX);
        assert_eq!(atoi(b"-99999999999"), i32::MIN);
        assert_eq!(atoi(b""), 0);
        assert_eq!(atoul(b"-1"), u32::MAX);
        assert_eq!(atoul(b"4294967296"), u32::MAX);
        assert_eq!(atoull(b"18446744073709551615"), u64::MAX);
        assert_eq!(atoull(b"+7"), 7);
    }

    #[test]
    fn reader_pads_with_zeros() {
        let r = Rd::new(b"\x01\x02abc");
        assert_eq!(r.u16(0), 0x0201);
        assert_eq!(r.u32(3), 0x6362);
        assert_eq!(r.s(2, 10), b"abc");
        assert_eq!(r.at(2).s(1, 1), b"b");
    }

    #[test]
    fn writer_safecopy() {
        let mut b = [9u8; 6];
        let mut w = W(&mut b);
        w.cstr(0, b"ab");
        w.cstr(4, b"");
        w.u16(5, 0x0102);
        assert_eq!(b, [b'a', b'b', 0, 9, 9, 2]);
    }

    #[test]
    fn strtok_cfg() {
        let cfg = b"login-server-port\t= 4000\nmysql-address\t\t= 127.0.0.1\r\n\npermitted-address\t= 1.2.3.4";
        let mut t = StrTok::new(cfg, b"= \t\n");
        let toks: Vec<Vec<u8>> = std::iter::from_fn(|| t.get()).collect();
        let want: Vec<&[u8]> = vec![b"login-server-port", b"4000", b"mysql-address", b"127.0.0.1", b"permitted-address", b"1.2.3.4"];
        assert_eq!(toks, want);
        let mut t = StrTok::new(b" x", b" ");
        assert_eq!(t.get(), Some(vec![]));
        assert_eq!(t.get(), Some(b"x".to_vec()));
        assert_eq!(t.get(), None);
    }

    #[test]
    fn date_past() {
        let now = [2026, 11, 19, 10, 30, 0];
        assert!(is_date_past(b"2026-11-19 10:29:59", now));
        assert!(!is_date_past(b"2026-12-19 18:00:21", now));
        assert!(is_date_past(b"2026-11-19 10:30:00", now));
        assert!(is_date_past(b"0000-00-00 00:00:00", now));
        assert!(!is_date_past(b"2099-01-01 00:00:00", now));
    }

    #[test]
    fn text_mode_read() {
        assert_eq!(text_mode(b"a\r\nb\rc\x1ad"), b"a\nb\rc");
        assert_eq!(text_mode(b"x\0y"), b"x");
    }
}
