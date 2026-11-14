use std::{collections::HashMap, hash::Hash, marker::PhantomData, sync::Arc};

#[derive(Debug)]
pub struct Handle<T> {
    id: u64,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<T> Eq for Handle<T> {}
impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Handle<T> {
    pub fn id(self) -> u64 {
        self.id
    }
}

#[derive(Debug)]
pub struct AssetManager<T, K = String> {
    assets: HashMap<Handle<T>, Arc<T>>,
    paths: HashMap<K, Handle<T>>,
    next_id: u64,
}

impl<T, K: Eq + Hash> Default for AssetManager<T, K> {
    fn default() -> Self {
        Self {
            assets: HashMap::new(),
            paths: HashMap::new(),
            next_id: 0,
        }
    }
}

impl<T, K: Eq + Hash> AssetManager<T, K> {
    pub fn insert(&mut self, key: K, asset: T) -> Handle<T> {
        let handle = Handle {
            id: self.next_id,
            _marker: PhantomData,
        };
        self.next_id += 1;
        self.assets.insert(handle, Arc::new(asset));
        self.paths.insert(key, handle);
        handle
    }

    pub fn replace(&mut self, key: &K, asset: T) -> Option<Handle<T>> {
        let handle = self.paths.get(key).copied()?;
        self.assets.insert(handle, Arc::new(asset));
        Some(handle)
    }

    pub fn get(&self, handle: Handle<T>) -> Option<Arc<T>> {
        self.assets.get(&handle).cloned()
    }

    pub fn get_ref(&self, handle: Handle<T>) -> Option<&T> {
        self.assets.get(&handle).map(Arc::as_ref)
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.assets.get_mut(&handle).and_then(Arc::get_mut)
    }

    pub fn load_cached<F>(&mut self, key: K, loader: F) -> Handle<T>
    where
        F: FnOnce() -> T,
    {
        if let Some(handle) = self.paths.get(&key).copied() {
            return handle;
        }
        self.insert(key, loader())
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asset_handles_are_cached() {
        let mut assets = AssetManager::<String>::default();
        let a = assets.load_cached("hero".into(), || "sprite".into());
        let b = assets.load_cached("hero".into(), || "wrong".into());
        assert_eq!(a, b);
        assert_eq!(assets.len(), 1);
        assert_eq!(&*assets.get(a).unwrap(), "sprite");
        let replacement = assets.replace(&"hero".into(), "updated".into());
        assert_eq!(replacement, Some(a));
        assert_eq!(assets.get_ref(a).map(String::as_str), Some("updated"));
        assert_eq!(assets.len(), 1);
    }

    #[test]
    fn replace_keeps_handle_stable() {
        let mut assets = AssetManager::<u32>::default();
        let handle = assets.insert("value".into(), 1);
        let replacement = assets.replace(&"value".into(), 2);
        assert_eq!(replacement, Some(handle));
        assert_eq!(assets.get(handle).as_deref(), Some(&2));
        assert_eq!(assets.len(), 1);
    }
}
