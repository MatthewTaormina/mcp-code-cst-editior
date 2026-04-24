use std::collections::HashMap;

/// A small generic container.
pub struct Bag<T: Clone> {
    items: Vec<T>,
    counts: HashMap<String, usize>,
}

impl<T: Clone> Bag<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            counts: HashMap::new(),
        }
    }

    pub fn push(&mut self, key: impl Into<String>, item: T) {
        self.items.push(item.clone());
        *self.counts.entry(key.into()).or_insert(0) += 1;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut b: Bag<i32> = Bag::new();
        b.push("a", 1);
        b.push("a", 2);
        assert_eq!(b.len(), 2);
    }
}
