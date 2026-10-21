//! mulberry32 and its helpers, draw for draw the same as src/engine/rng.ts.

#[derive(Clone, Copy, Debug)]
pub struct Rng {
    s: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self { s: seed }
    }

    /// Serializable state; `Rng::new(state())` resumes the sequence.
    pub fn state(&self) -> u32 {
        self.s
    }

    /// Uniform float in [0, 1).
    #[inline]
    pub fn next(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x6d2b_79f5);
        let mut t = self.s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }

    #[inline]
    pub fn int(&mut self, n: usize) -> usize {
        (self.next() * n as f64).floor() as usize
    }

    #[inline]
    pub fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }

    /// None (and no draw) when empty.
    #[inline]
    pub fn pick<T: Copy>(&mut self, arr: &[T]) -> Option<T> {
        if arr.is_empty() {
            None
        } else {
            Some(arr[self.int(arr.len())])
        }
    }

    /// Forward Fisher–Yates in place: j ∈ [i, len).
    pub fn shuffle<T>(&mut self, a: &mut [T]) {
        let n = a.len();
        if n < 2 {
            return;
        }
        for i in 0..n - 1 {
            let j = i + self.int(n - i);
            a.swap(i, j);
        }
    }

    /// Shuffles all of `a` (every draw, as the TypeScript does), then keeps the first `n`.
    pub fn sample_in_place<T>(&mut self, a: &mut [T], n: usize) -> usize {
        self.shuffle(a);
        n.min(a.len())
    }

    /// One item, each with chance weight / total weight.
    pub fn weighted<T: Copy>(&mut self, arr: &[T], weight: impl Fn(T) -> f64) -> T {
        let mut total = 0.0;
        for &x in arr {
            total += weight(x).max(0.0);
        }
        let mut r = self.next() * total;
        for &x in arr {
            r -= weight(x).max(0.0);
            if r < 0.0 {
                return x;
            }
        }
        arr[arr.len() - 1]
    }
}
