use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use parking_lot::{Mutex, RwLock};

pub(crate) struct Reloadable<T> {
    state: RwLock<(u32, Option<Arc<T>>)>,
    init: fn() -> T,
}

impl<T> Reloadable<T> {
    pub(crate) const fn new(init: fn() -> T) -> Self {
        Self {
            state: RwLock::new((0, None)),
            init,
        }
    }

    pub(crate) fn get(&self) -> Arc<T> {
        let epoch = {
            let state = self.state.read();
            if let Some(value) = &state.1 {
                return value.clone();
            }
            state.0
        };
        let built = Arc::new((self.init)());
        let mut state = self.state.write();
        if state.0 != epoch {
            return built;
        }
        state.1.get_or_insert(built).clone()
    }

    pub(crate) fn reset(&self) {
        let mut state = self.state.write();
        state.0 = state.0.wrapping_add(1);
        state.1 = None;
    }

    pub(crate) fn epoch(&self) -> u32 {
        self.state.read().0
    }
}

pub(crate) struct Label {
    resolved: AtomicU32,
    text: AtomicPtr<String>,
}

impl Label {
    pub(crate) const fn new() -> Self {
        Self {
            resolved: AtomicU32::new(0),
            text: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    pub(crate) fn get(&self, epoch: u32, resolve: impl FnOnce() -> String) -> &'static str {
        let want = epoch.wrapping_add(1);
        if self.resolved.load(Ordering::Acquire) == want {
            return unsafe { &*self.text.load(Ordering::Relaxed) }.as_str();
        }
        let text = intern(resolve());
        self.text
            .store(std::ptr::from_ref(text).cast_mut(), Ordering::Relaxed);
        self.resolved.store(want, Ordering::Release);
        text.as_str()
    }
}

fn intern(text: String) -> &'static String {
    static SEEN: Mutex<Option<HashMap<String, &'static String>>> = Mutex::new(None);
    let mut seen = SEEN.lock();
    let seen = seen.get_or_insert_with(HashMap::new);
    if let Some(known) = seen.get(&text) {
        return known;
    }
    let leaked: &'static String = Box::leak(Box::new(text.clone()));
    seen.insert(text, leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static BUILDS: AtomicUsize = AtomicUsize::new(0);

    fn count() -> usize {
        BUILDS.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[test]
    fn a_value_is_built_once_until_reset() {
        static VALUE: Reloadable<usize> = Reloadable::new(count);
        let first = *VALUE.get();
        assert_eq!(*VALUE.get(), first);
        VALUE.reset();
        assert_eq!(*VALUE.get(), first + 1);
    }

    #[test]
    fn a_label_resolves_again_for_a_new_epoch() {
        static LABEL: Label = Label::new();
        assert_eq!(LABEL.get(0, || "a".into()), "a");
        assert_eq!(LABEL.get(0, || "b".into()), "a");
        assert_eq!(LABEL.get(1, || "b".into()), "b");
        assert!(std::ptr::eq(
            LABEL.get(1, || unreachable!()),
            intern("b".into()).as_str()
        ));
    }
}
