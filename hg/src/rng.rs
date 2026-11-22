//! MSVC CRT `rand()` / `srand()`: x = x*214013 + 2531011, result (x >> 16) & 0x7FFF. The C++ HG draws every
//! random number from this one global state, so a single `Rng` owned by the world reproduces its sequence.

pub const RAND_MAX: i32 = 0x7FFF;

pub struct Rng(u32, Option<f64>);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed, None)
    }
    pub fn srand(&mut self, seed: u32) {
        self.0 = seed;
    }
    pub fn rand(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        ((self.0 >> 16) & 0x7FFF) as i32
    }
    /// Math.h `ndice`: sum of `throw` rolls of 1..=range.
    pub fn ndice(&mut self, throw: i32, range: i32) -> i32 {
        if range <= 0 || throw <= 0 {
            return 0;
        }
        (0..throw).map(|_| self.rand() % range + 1).sum()
    }
    /// Math.h `randn_notrig` (polar Box-Muller; the second deviate is kept for the next call, like its statics).
    pub fn randn(&mut self) -> f64 {
        if let Some(d) = self.1.take() {
            return d;
        }
        loop {
            let v1 = 2.0 * (self.rand() as f64 / RAND_MAX as f64) - 1.0;
            let v2 = 2.0 * (self.rand() as f64 / RAND_MAX as f64) - 1.0;
            let r = v1 * v1 + v2 * v2;
            if r < 1.0 && r != 0.0 {
                let polar = (-2.0 * r.ln() / r).sqrt();
                self.1 = Some(v1 * polar);
                return v2 * polar;
            }
        }
    }
    /// Math.h `dice`: above 10 throws a normal approximation instead of rolling every die.
    pub fn dice(&mut self, throw: i32, range: i32) -> i32 {
        if throw > 10 {
            let t = throw as f64;
            // the C++ adds int and double terms: (throw*range/2) + (throw*randn/3) + (throw/2), truncated
            return ((throw * range / 2) as f64 + t * self.randn() / 3.0 + (throw / 2) as f64) as i32;
        }
        self.ndice(throw, range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msvc_sequence() {
        // srand(1) on MSVC: 41, 18467, 6334, 26500, 19169
        let mut r = Rng::new(1);
        let v: Vec<i32> = (0..5).map(|_| r.rand()).collect();
        assert_eq!(v, [41, 18467, 6334, 26500, 19169]);
        r.srand(0);
        assert_eq!(r.rand(), 38);
    }

    #[test]
    fn dice_range() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let d = r.dice(2, 4);
            assert!((2..=8).contains(&d));
        }
        assert_eq!(r.dice(0, 4), 0);
        let big: Vec<i32> = (0..200).map(|_| r.dice(100000, 4)).collect();
        let mean = big.iter().map(|&d| d as f64).sum::<f64>() / 200.0;
        assert!((240000.0..260000.0).contains(&mean), "normal around 250000, got {mean}");
    }
}
