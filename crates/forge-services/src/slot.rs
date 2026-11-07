use std::sync::RwLock;

/// A process-wide value set by the host at launch: what
/// `native::NativeServices::installed()` reads.
pub struct Slot<T> {
    value: RwLock<Option<T>>,
}

impl<T: Clone> Slot<T> {
    pub const fn new() -> Self {
        Self {
            value: RwLock::new(None),
        }
    }

    pub fn get(&self) -> Option<T> {
        self.value
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn set(&self, value: T) {
        *self
            .value
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(value);
    }

    pub fn clear(&self) {
        *self
            .value
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

impl<T: Clone> Default for Slot<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear() {
        static SLOT: Slot<u32> = Slot::new();
        assert_eq!(SLOT.get(), None);
        SLOT.set(3);
        SLOT.set(4);
        assert_eq!(SLOT.get(), Some(4));
        SLOT.clear();
        assert_eq!(SLOT.get(), None);
    }
}
