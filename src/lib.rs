//! This crate provides a time sensitive key-value cache.  When an item is inserted it is
//! given a TTL.  Any value that are in the cache after their duration are considered invalid
//! and will not be returned on lookups.

extern crate linked_hash_map;

use std::borrow::Borrow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash};
#[cfg(feature = "stats")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use linked_hash_map::LinkedHashMap;
use linked_hash_map::Entry as LinkedHashMapEntry;
use linked_hash_map::OccupiedEntry as OccupiedLinkHashMapEntry;
use linked_hash_map::VacantEntry as VacantLinkHashMapEntry;

/// A view into a single location in a map, which may be vacant or occupied.
pub enum Entry<'a, K: 'a, V: 'a, S: 'a = RandomState> {
    /// An occupied Entry.
    Occupied(OccupiedEntry<'a, K, V, S>),
    /// A vacant Entry.
    Vacant(VacantEntry<'a, K, V, S>),
}

impl<'a, K: Hash + Eq, V, S: BuildHasher> Entry<'a, K, V, S> {
    pub fn key(&self) -> &K {
        match *self {
            Entry::Occupied(ref e) => e.key(),
            Entry::Vacant(ref e) => e.key(),
        }
    }
}

/// A view into a single occupied location in the cache that was unexpired at the moment of lookup.
pub struct OccupiedEntry<'a, K: 'a, V: 'a, S: 'a = RandomState> {
    entry: OccupiedLinkHashMapEntry<'a, K, InternalEntry<V>, S>
}

impl<'a, K: Hash + Eq, V, S: BuildHasher> OccupiedEntry<'a, K, V, S> {
    /// Gets a reference to the entry key
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut map = TtlCache::new(10);
    ///
    /// map.insert("foo".to_string(), 1, Duration::from_secs(30));
    /// assert_eq!("foo", map.entry("foo".to_string()).key());
    /// ```
    pub fn key(&self) -> &K {
        self.entry.key()
    }

    /// Gets a reference to the value in the entry.
    pub fn get(&self) -> &V {
        &self.entry.get().value
    }

    /// Gets a mutable reference to the value in the entry.
    pub fn get_mut(&mut self) -> &mut V {
        &mut self.entry.get_mut().value
    }

    /// Sets the value of the entry, and returns the entry's old value
    pub fn insert(&mut self, value: V, duration: Duration) -> V {
        let internal_entry = self.entry.insert(InternalEntry::new(value, duration));
        internal_entry.value
    }
}



/// A view into a single empty location in the cache
pub struct VacantEntry<'a, K: 'a, V: 'a, S: 'a = RandomState> {
    entry: VacantLinkHashMapEntry<'a, K, InternalEntry<V>, S>
}

impl<'a, K: 'a + Hash + Eq, V: 'a, S: BuildHasher> VacantEntry<'a, K, V, S> {
    /// Gets a reference to the entry key
    ///
    /// # Examples
    ///
    /// ```
    /// use ttl_cache::TtlCache;
    ///
    /// let mut map = TtlCache::<String, u32>::new(10);
    ///
    /// assert_eq!("foo", map.entry("foo".to_string()).key());
    /// ```
    pub fn key(&self) -> &K {
        self.entry.key()
    }

    /// Sets the value of the entry with the VacantEntry's key,
    /// and returns a mutable reference to it
    pub fn insert(self, value: V, duration: Duration) -> &'a mut V {
        let internal_entry = self.entry.insert(InternalEntry::new(value, duration));
        &mut internal_entry.value
    }
}

#[derive(Clone)]
struct InternalEntry<V> {
    value: V,
    expiration: Instant,
    duration: Duration,
}

impl<V> InternalEntry<V> {
    fn new(v: V, duration: Duration) -> Self {
        InternalEntry {
            value: v,
            expiration: Instant::now() + duration,
            duration
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.expiration
    }

    fn reset_duration(&mut self) {
        self.expiration = Instant::now() + self.duration
    }
}

/// A time sensitive cache.
pub struct TtlCache<K: Eq + Hash, V, S: BuildHasher = RandomState> {
    map: LinkedHashMap<K, InternalEntry<V>, S>,
    #[cfg(feature = "stats")]
    hits: AtomicUsize,
    #[cfg(feature = "stats")]
    misses: AtomicUsize,
    #[cfg(feature = "stats")]
    since: Instant,
}

impl<K: Eq + Hash, V> TtlCache<K, V> {
    /// Creates an empty cache
    ///
    /// # Examples
    ///
    /// ```
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache: TtlCache<i32, &str> = TtlCache::new();
    /// ```
    pub fn new() -> Self {
        TtlCache {
            map: LinkedHashMap::new(),
            #[cfg(feature = "stats")]
            hits: AtomicUsize::new(0),
            #[cfg(feature = "stats")]
            misses: AtomicUsize::new(0),
            #[cfg(feature = "stats")]
            since: Instant::now(),
        }
    }
}

/// Creates an empty cache as the default
impl<K: Eq + Hash, V> Default for TtlCache<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash, V, S: BuildHasher> TtlCache<K, V, S> {
    /// Creates an empty cache that can hold at most `capacity` items
    /// with the given hash builder.
    pub fn with_hasher(hash_builder: S) -> Self {
        TtlCache {
            map: LinkedHashMap::with_hasher(hash_builder),
            #[cfg(feature = "stats")]
            hits: AtomicUsize::new(0),
            #[cfg(feature = "stats")]
            misses: AtomicUsize::new(0),
            #[cfg(feature = "stats")]
            since: Instant::now(),
        }
    }

    /// Check if the cache contains the given key.
    ///
    /// # Examples
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(10);
    /// cache.insert(1, "a", Duration::from_secs(30));
    /// assert_eq!(cache.contains_key(&1), true);
    /// ```
    pub fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        // Expiration check is handled by get
        self.get(key).is_some()
    }

    /// Inserts a key-value pair into the cache with an individual ttl for the key. If the key
    /// already existed and hasn't expired, the old value is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    ///
    /// cache.insert(1, "a", Duration::from_secs(20));
    /// cache.insert(2, "b", Duration::from_secs(60));
    /// assert_eq!(cache.get(&1), Some(&"a"));
    /// assert_eq!(cache.get(&2), Some(&"b"));
    /// ```
    pub fn insert(&mut self, k: K, v: V, ttl: Duration) -> Option<V> {
        self.remove_expired();
        let to_insert = InternalEntry::new(v, ttl);
        let old_val = self.map.insert(k, to_insert);
        old_val.and_then(|x| if x.is_expired() { None } else { Some(x.value) })
    }

    /// Returns a reference to the value corresponding to the given key in the cache, if
    /// it contains an unexpired entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    /// let duration = Duration::from_secs(30);
    ///
    /// cache.insert(1, "a", duration);
    /// cache.insert(2, "b", duration);
    /// cache.insert(2, "c", duration);
    /// cache.insert(3, "d", duration);
    ///
    /// assert_eq!(cache.get(&1), None);
    /// assert_eq!(cache.get(&2), Some(&"c"));
    /// ```
    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let to_ret = self.map
            .get(k)
            .and_then(|x| if x.is_expired() { None } else { Some(&x.value) });
        #[cfg(feature = "stats")]
        {
            if to_ret.is_some() {
                self.hits.fetch_add(1, Ordering::Relaxed);
            } else {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        to_ret
    }

    /// Returns a mutable reference to the value corresponding to the given key in the cache, if
    /// it contains an unexpired entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    /// let duration = Duration::from_secs(30);
    ///
    /// cache.insert(1, "a", duration);
    /// cache.insert(2, "b", duration);
    /// cache.insert(2, "c", duration);
    /// cache.insert(3, "d", duration);
    ///
    /// assert_eq!(cache.get_mut(&1), None);
    /// assert_eq!(cache.get_mut(&2), Some(&mut "c"));
    /// ```
    pub fn get_mut<Q: ?Sized>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let to_ret = self.map.get_mut(k).and_then(|x| {
            if x.is_expired() {
                None
            } else {
                Some(&mut x.value)
            }
        });
        #[cfg(feature = "stats")]
        {
            if to_ret.is_some() {
                self.hits.fetch_add(1, Ordering::Relaxed);
            } else {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        to_ret
    }

    /// Returns a mutable reference to the value corresponding to the given key in the cache, if
    /// it contains an unexpired entry and resets the expiration.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    /// let duration = Duration::from_secs(30);
    ///
    /// cache.insert(1, "a", duration);
    /// cache.insert(2, "b", duration);
    /// cache.insert(2, "c", duration);
    /// cache.insert(3, "d", duration);
    ///
    /// assert_eq!(cache.get_mut_prolong(&1), None);
    /// assert_eq!(cache.get_mut_prolong(&2), Some(&mut "c"));
    /// ```
    pub fn get_mut_prolong<Q: ?Sized>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let to_ret = self.map.get_mut(k).and_then(|x| {
            if x.is_expired() {
                None
            } else {
                x.reset_duration();
                Some(&mut x.value)
            }
        });
        #[cfg(feature = "stats")]
        {
            if to_ret.is_some() {
                self.hits.fetch_add(1, Ordering::Relaxed);
            } else {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        to_ret
    }

    /// Sets the expiration of the entry pointed to by the given key to
    /// now + the originally given duration
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    ///
    /// cache.insert(2, "a", Duration::from_secs(30));
    /// 
    /// cache.reset_ttl(&2)
    /// ```
    pub fn reset_ttl<Q: ?Sized>(&mut self, k: &Q)
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        if let Some(entry) = self.map.get_mut(k) {
            if !entry.is_expired() {
                entry.reset_duration()
            }
        }
    }

    /// Removes the given key from the cache and returns its corresponding value.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    ///
    /// cache.insert(2, "a", Duration::from_secs(30));
    ///
    /// assert_eq!(cache.remove(&1), None);
    /// assert_eq!(cache.remove(&2), Some("a"));
    /// assert_eq!(cache.remove(&2), None);
    /// ```
    pub fn remove<Q: ?Sized>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.map
            .remove(k)
            .and_then(|x| if x.is_expired() { None } else { Some(x.value) })
    }

    /// Clears all values out of the cache
    pub fn clear(&mut self) {
        self.map.clear();
    }


    pub fn entry(&mut self, k: K) -> Entry<K, V, S> {
        let should_remove = self.map.get(&k).map(|value| value.is_expired()).unwrap_or(false);
        if should_remove {
            self.map.remove(&k);
        }
        match self.map.entry(k){
            LinkedHashMapEntry::Occupied(entry) => {
                Entry::Occupied(OccupiedEntry {
                    entry
                })
            }
            LinkedHashMapEntry::Vacant(entry) => {
                Entry::Vacant(VacantEntry{
                    entry
                })
            }
        }
    }

    /// Returns an iterator over the cache's key-value pairs in oldest to youngest order.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    /// let duration = Duration::from_secs(30);
    ///
    /// cache.insert(1, 10, duration);
    /// cache.insert(2, 20, duration);
    /// cache.insert(3, 30, duration);
    ///
    /// let kvs: Vec<_> = cache.iter().collect();
    /// assert_eq!(kvs, [(&2, &20), (&3, &30)]);
    /// ```
    pub fn iter(&mut self) -> Iter<K, V> {
        self.remove_expired();
        Iter(self.map.iter())
    }

    /// Returns an iterator over the cache's key-value pairs in oldest to youngest order with
    /// mutable references to the values.
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    /// let duration = Duration::from_secs(30);
    ///
    /// cache.insert(1, 10, duration);
    /// cache.insert(2, 20, duration);
    /// cache.insert(3, 30, duration);
    ///
    /// let mut n = 2;
    ///
    /// for (k, v) in cache.iter_mut() {
    ///     assert_eq!(*k, n);
    ///     assert_eq!(*v, n * 10);
    ///     *v *= 10;
    ///     n += 1;
    /// }
    ///
    /// assert_eq!(n, 4);
    /// assert_eq!(cache.get(&2), Some(&200));
    /// assert_eq!(cache.get(&3), Some(&300));
    /// ```
    pub fn iter_mut(&mut self) -> IterMut<K, V> {
        self.remove_expired();
        IterMut(self.map.iter_mut())
    }

    /// The cache will keep track of some basic stats during its usage that can be helpful
    /// for performance tuning or monitoring.  This method will reset these counters.
    /// # Examples
    ///
    /// ```
    /// use std::thread::sleep;
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    ///
    /// cache.insert(1, "a", Duration::from_secs(20));
    /// cache.insert(2, "b", Duration::from_millis(1));
    /// sleep(Duration::from_millis(10));
    /// let _ = cache.get(&1);
    /// let _ = cache.get(&2);
    /// let _ = cache.get(&3);
    /// assert_eq!(cache.miss_count(), 2);
    /// cache.reset_stats_counter();
    /// assert_eq!(cache.miss_count(), 0);
    #[cfg(feature = "stats")]
    pub fn reset_stats_counter(&mut self) {
        self.hits = AtomicUsize::new(0);
        self.misses = AtomicUsize::new(0);
        self.since = Instant::now();
    }

    /// Returns the number of unexpired cache hits since the last time the counters were reset.
    /// # Examples
    ///
    /// ```
    /// use std::thread::sleep;
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    ///
    /// cache.insert(1, "a", Duration::from_secs(20));
    /// cache.insert(2, "b", Duration::from_millis(1));
    /// sleep(Duration::from_millis(10));
    /// assert!(cache.get(&1).is_some());
    /// assert!(cache.get(&2).is_none());
    /// assert!(cache.get(&3).is_none());
    /// assert_eq!(cache.hit_count(), 1);
    #[cfg(feature = "stats")]
    pub fn hit_count(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    /// Returns the number of cache misses since the last time the counters were reset.  Entries
    /// that have expired count as a miss.
    /// # Examples
    ///
    /// ```
    /// use std::thread::sleep;
    /// use std::time::Duration;
    /// use ttl_cache::TtlCache;
    ///
    /// let mut cache = TtlCache::new(2);
    ///
    /// cache.insert(1, "a", Duration::from_secs(20));
    /// cache.insert(2, "b", Duration::from_millis(1));
    /// sleep(Duration::from_millis(10));
    /// let _ = cache.get(&1);
    /// let _ = cache.get(&2);
    /// let _ = cache.get(&3);
    /// assert_eq!(cache.miss_count(), 2);
    #[cfg(feature = "stats")]
    pub fn miss_count(&self) -> usize {
        self.misses.load(Ordering::Relaxed)
    }

    /// Returns the Instant when we started gathering stats.  This is either when the cache was
    /// created or when it was last reset, whichever happened most recently.
    #[cfg(feature = "stats")]
    pub fn stats_since(&self) -> Instant {
        self.since
    }

    pub fn remove_expired(&mut self) {
        let should_pop_head = |map: &LinkedHashMap<K, InternalEntry<V>, S>| match map.front() {
            Some(entry) => entry.1.is_expired(),
            None => false,
        };
        while should_pop_head(&self.map) {
            self.map.pop_front();
        }
    }
}

impl<K: Eq + Hash, V> Clone for TtlCache<K, V>
where
    K: Clone,
    V: Clone,
{
    fn clone(&self) -> TtlCache<K, V> {
        TtlCache {
            map: self.map.clone(),
            #[cfg(feature = "stats")]
            hits: AtomicUsize::new(self.hits.load(Ordering::Relaxed)),
            #[cfg(feature = "stats")]
            misses: AtomicUsize::new(self.misses.load(Ordering::Relaxed)),
            #[cfg(feature = "stats")]
            since: self.since,
        }
    }
}

pub struct Iter<'a, K: 'a, V: 'a>(linked_hash_map::Iter<'a, K, InternalEntry<V>>);

impl<'a, K, V> Clone for Iter<'a, K, V> {
    fn clone(&self) -> Iter<'a, K, V> {
        Iter(self.0.clone())
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<(&'a K, &'a V)> {
        match self.0.next() {
            Some(entry) => {
                if entry.1.is_expired() {
                    self.next()
                } else {
                    Some((entry.0, &entry.1.value))
                }
            }
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a, K, V> DoubleEndedIterator for Iter<'a, K, V> {
    fn next_back(&mut self) -> Option<(&'a K, &'a V)> {
        match self.0.next_back() {
            Some(entry) => {
                if entry.1.is_expired() {
                    // The entries are in order of time.  So if the previous entry is expired, every
                    // else before it will be expired too.
                    None
                } else {
                    Some((entry.0, &entry.1.value))
                }
            }
            None => None,
        }
    }
}

pub struct IterMut<'a, K: 'a, V: 'a>(linked_hash_map::IterMut<'a, K, InternalEntry<V>>);

impl<'a, K, V> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);
    fn next(&mut self) -> Option<(&'a K, &'a mut V)> {
        match self.0.next() {
            Some(entry) => {
                if entry.1.is_expired() {
                    self.next()
                } else {
                    Some((entry.0, &mut entry.1.value))
                }
            }
            None => None,
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a, K, V> DoubleEndedIterator for IterMut<'a, K, V> {
    fn next_back(&mut self) -> Option<(&'a K, &'a mut V)> {
        match self.0.next_back() {
            Some(entry) => {
                if entry.1.is_expired() {
                    None
                } else {
                    Some((entry.0, &mut entry.1.value))
                }
            }
            None => None,
        }
    }
}
