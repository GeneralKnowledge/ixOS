//! A value commands replace and queries read without waiting.

use std::sync::{Arc, RwLock};

/// What queries read: a committed value that commands replace whole. A
/// query never waits for a command or for disk; it gets the last value a
/// command published. Optional: apps choose what, if anything, to publish.
#[derive(Debug)]
pub struct Published<V> {
    value: RwLock<Arc<V>>,
}

impl<V> Published<V> {
    pub fn new(value: V) -> Self {
        Self {
            value: RwLock::new(Arc::new(value)),
        }
    }

    /// The current value. Cheap: an `Arc` clone under a short read lock.
    pub fn get(&self) -> Arc<V> {
        Arc::clone(&self.value.read().unwrap_or_else(|p| p.into_inner()))
    }

    /// Replaces the value. Call it from a command, after the commit.
    pub fn set(&self, value: V) {
        *self.value.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(value);
    }
}

impl<V: Default> Default for Published<V> {
    fn default() -> Self {
        Self::new(V::default())
    }
}
