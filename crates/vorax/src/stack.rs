//! A fixed-capacity list that lives on the stack, so game states stay `Copy` and the hot loops never allocate.

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StackVec<T: Copy + Default, const N: usize> {
    items: [T; N],
    len: u8,
}

impl<T: Copy + Default, const N: usize> Default for StackVec<T, N> {
    fn default() -> Self {
        Self { items: [T::default(); N], len: 0 }
    }
}

impl<T: Copy + Default, const N: usize> StackVec<T, N> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_slice(s: &[T]) -> Self {
        let mut v = Self::new();
        for &x in s {
            v.push(x);
        }
        v
    }

    #[inline]
    pub fn push(&mut self, x: T) {
        assert!((self.len as usize) < N, "StackVec capacity {N} exceeded");
        self.items[self.len as usize] = x;
        self.len += 1;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len as usize]
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items[..self.len as usize]
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<T> {
        self.as_slice().get(i).copied()
    }

    pub fn truncate(&mut self, n: usize) {
        if n < self.len as usize {
            self.len = n as u8;
        }
    }

    pub fn remove(&mut self, i: usize) -> T {
        let x = self.items[i];
        let n = self.len as usize;
        self.items.copy_within(i + 1..n, i);
        self.len -= 1;
        x
    }

    pub fn contains(&self, x: &T) -> bool
    where
        T: PartialEq,
    {
        self.as_slice().contains(x)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

impl<T: Copy + Default + std::fmt::Debug, const N: usize> std::fmt::Debug for StackVec<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl<T: Copy + Default, const N: usize> std::ops::Index<usize> for StackVec<T, N> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.as_slice()[i]
    }
}

impl<'a, T: Copy + Default, const N: usize> IntoIterator for &'a StackVec<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
