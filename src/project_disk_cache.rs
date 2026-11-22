//! Short-lived legacy disk-history snapshots, never policy or write authority.
//!
//! The class aggregate is still read live before this cache is consulted. Only
//! the retained legacy floor is cached, for at most 60 seconds without sliding
//! expiry. New class observations therefore still raise the conservative bound.
//! Failed/incomplete reads are never stored or served as an empty history.

use super::DiskSample;
use crate::errors::Result;

pub(super) fn get_or_fetch(
    endpoint: &str,
    signature: &str,
    filesystem: &str,
    fetch: impl FnOnce() -> Result<Vec<DiskSample>>,
) -> Result<Vec<DiskSample>> {
    #[cfg(target_os = "linux")]
    {
        // Unit callers must not publish into the operator cache. The core
        // below is tested with explicit private paths, including real locks.
        let root = if cfg!(test) {
            None
        } else {
            crate::projection_cache::cache_path()
                .and_then(|p| p.parent().map(|p| p.join("disk-legacy")))
        };
        linux::cached(
            root.as_deref(),
            endpoint,
            signature,
            filesystem,
            crate::projection_cache::now_secs(),
            fetch,
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (endpoint, signature, filesystem);
        fetch()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::errors::Error;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::Path;

    const TTL: u64 = 60;
    const MAX_BYTES: u64 = 1024 * 1024;
    const MAX_KEYS: usize = 128;

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Entry {
        at: u64,
        samples: Vec<DiskSample>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Snapshot {
        version: u8,
        entries: BTreeMap<String, Entry>,
    }

    fn empty() -> Snapshot {
        Snapshot {
            version: 1,
            entries: BTreeMap::new(),
        }
    }

    fn private(path: &Path, uid: u32, directory: bool) -> bool {
        fs::symlink_metadata(path).is_ok_and(|m| {
            m.uid() == uid
                && m.mode() & 0o077 == 0
                && if directory {
                    m.is_dir()
                } else {
                    m.is_file() && m.nlink() == 1
                }
        })
    }

    fn open_private(path: &Path, uid: u32) -> Option<File> {
        if !private(path, uid, false) {
            return None;
        }
        let file = File::open(path).ok()?;
        let before = fs::symlink_metadata(path).ok()?;
        let opened = file.metadata().ok()?;
        (before.dev() == opened.dev()
            && before.ino() == opened.ino()
            && opened.uid() == uid
            && opened.mode() & 0o077 == 0
            && opened.is_file()
            && opened.nlink() == 1)
            .then_some(file)
    }

    fn load(root: &Path, uid: u32) -> Snapshot {
        let value = (|| {
            let file = open_private(&root.join("snapshots.json"), uid)?;
            if file.metadata().ok()?.len() > MAX_BYTES {
                return None;
            }
            let mut raw = Vec::new();
            file.take(MAX_BYTES + 1).read_to_end(&mut raw).ok()?;
            if raw.len() as u64 > MAX_BYTES {
                return None;
            }
            let snapshot: Snapshot = serde_json::from_slice(&raw).ok()?;
            (snapshot.version == 1
                && snapshot.entries.len() <= MAX_KEYS
                && snapshot.entries.iter().all(|(key, entry)| {
                    key.len() == 64
                        && key.bytes().all(|b| b.is_ascii_hexdigit())
                        && entry.samples.len() <= 100
                }))
            .then_some(snapshot)
        })();
        value.unwrap_or_else(empty)
    }

    fn hit(snapshot: &Snapshot, key: &str, now: u64) -> Option<Vec<DiskSample>> {
        let entry = snapshot.entries.get(key)?;
        (entry.at <= now && now - entry.at < TTL).then(|| {
            #[cfg(not(test))]
            crate::metrics::emit(
                "disk_legacy_snapshot",
                &[
                    ("source", "cache".into()),
                    ("age_secs", (now - entry.at).into()),
                    ("samples", entry.samples.len().into()),
                ],
            );
            entry.samples.clone()
        })
    }

    pub(super) fn cached(
        root: Option<&Path>,
        endpoint: &str,
        signature: &str,
        filesystem: &str,
        now: u64,
        fetch: impl FnOnce() -> Result<Vec<DiskSample>>,
    ) -> Result<Vec<DiskSample>> {
        let Some(root) = root else {
            return fetch();
        };
        let Ok(meta) = fs::metadata("/proc/self") else {
            return fetch();
        };
        let uid = meta.uid();
        let _ = fs::DirBuilder::new().mode(0o700).create(root);
        if !private(root, uid, true) {
            return fetch();
        }
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&(endpoint.trim_end_matches('/'), signature, filesystem))
                .expect("string tuple serialization"),
        ));
        if let Some(samples) = hit(&load(root, uid), &key, now) {
            return Ok(samples);
        }

        let lock_path = root.join("refresh.lock");
        let created = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock_path);
        drop(created);
        let Some(lock) = open_private(&lock_path, uid) else {
            return fetch();
        };
        if lock.try_lock().is_err() {
            // No parallel live fallback: this is advisory evidence, and an
            // in-progress refresh is UNKNOWN rather than a lower disk bound.
            return Err(Error::Unavailable(
                "legacy disk-history refresh in progress".into(),
            ));
        }
        let mut snapshot = load(root, uid);
        if let Some(samples) = hit(&snapshot, &key, now) {
            return Ok(samples);
        }
        let samples = fetch()?;
        #[cfg(not(test))]
        crate::metrics::emit(
            "disk_legacy_snapshot",
            &[("source", "live".into()), ("samples", samples.len().into())],
        );
        if samples.len() > 100 {
            return Ok(samples);
        }
        if snapshot.entries.len() >= MAX_KEYS && !snapshot.entries.contains_key(&key)
            && let Some(oldest) = snapshot.entries.iter().min_by_key(|(_, e)| e.at)
                .map(|(k, _)| k.clone())
        {
            snapshot.entries.remove(&oldest);
        }
        snapshot.entries.insert(
            key,
            Entry {
                at: now,
                samples: samples.clone(),
            },
        );
        // Cache persistence never changes the already-observed answer. Atomic
        // rename plus the refresh lock prevents torn data and lost key updates.
        let _ = (|| -> std::io::Result<()> {
            let raw = serde_json::to_vec(&snapshot)?;
            if raw.len() as u64 > MAX_BYTES {
                return Ok(());
            }
            let mut file = tempfile::NamedTempFile::new_in(root)?;
            file.write_all(&raw)?;
            file.persist(root.join("snapshots.json"))
                .map_err(|e| e.error)?;
            Ok(())
        })();
        Ok(samples)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;
        use std::os::unix::fs::PermissionsExt;

        fn samples(n: i64) -> Vec<DiskSample> {
            vec![DiskSample { delta_bytes: n }]
        }

        #[test]
        fn repeated_legacy_reads_are_one_fetch_until_strict_expiry() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            let count = Cell::new(0);
            let fetch = || {
                count.set(count.get() + 1);
                Ok(samples(17))
            };
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 100, fetch).unwrap(),
                samples(17)
            );
            for now in 101..160 {
                assert_eq!(
                    cached(Some(&root), "e", "s", "f", now, fetch).unwrap(),
                    samples(17)
                );
            }
            assert_eq!(count.get(), 1);
            cached(Some(&root), "e", "s", "f", 160, fetch).unwrap();
            assert_eq!(count.get(), 2);
        }

        #[test]
        fn endpoint_signature_filesystem_and_clock_are_bound() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            for (e, s, f, n) in [
                ("e", "s", "f", 1),
                ("other", "s", "f", 2),
                ("e", "other", "f", 3),
                ("e", "s", "other", 4),
            ] {
                assert_eq!(
                    cached(Some(&root), e, s, f, 100, || Ok(samples(n))).unwrap(),
                    samples(n)
                );
            }
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 101, || panic!("cache missed")).unwrap(),
                samples(1)
            );
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 99, || Ok(samples(5))).unwrap(),
                samples(5)
            );
        }

        #[test]
        fn errors_are_not_cached_or_replaced_by_an_expired_answer() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            cached(Some(&root), "e", "s", "f", 100, || Ok(samples(9))).unwrap();
            assert!(
                cached(Some(&root), "e", "s", "f", 160, || Err(Error::Unavailable(
                    "fixture".into()
                )))
                .is_err()
            );
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 161, || Ok(samples(12))).unwrap(),
                samples(12)
            );
        }

        #[test]
        fn successful_empty_history_is_cached_but_corruption_is_not() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            assert!(cached(Some(&root), "e", "s", "f", 100, || Ok(vec![]))
                .unwrap()
                .is_empty());
            assert!(
                cached(Some(&root), "e", "s", "f", 101, || panic!("empty miss"))
                    .unwrap()
                    .is_empty()
            );
            fs::write(root.join("snapshots.json"), b"{").unwrap();
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 102, || Ok(samples(2))).unwrap(),
                samples(2)
            );
        }

        #[test]
        fn real_refresh_lock_does_not_fan_out_live_queries() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            cached(Some(&root), "e", "s", "f", 100, || Ok(samples(1))).unwrap();
            let lock = File::open(root.join("refresh.lock")).unwrap();
            lock.try_lock().unwrap();
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 101, || panic!("warm fetch")).unwrap(),
                samples(1)
            );
            assert!(cached(Some(&root), "e", "s", "f", 160, || panic!("parallel fetch")).is_err());
            drop(lock);
            cached(Some(&root), "e", "s", "f", 160, || Ok(samples(2))).unwrap();
        }

        #[test]
        fn cache_cardinality_and_bytes_stay_bounded() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            for n in 0..150 {
                cached(Some(&root), "e", &n.to_string(), "f", n, || Ok(samples(1))).unwrap();
            }
            let uid = fs::metadata("/proc/self").unwrap().uid();
            assert_eq!(load(&root, uid).entries.len(), MAX_KEYS);
            assert!(fs::metadata(root.join("snapshots.json")).unwrap().len() <= MAX_BYTES);
            assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        }

        #[test]
        fn symlink_and_shared_cache_paths_cannot_publish_or_serve() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("cache");
            let target = dir.path().join("untouched");
            fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(&target, &root).unwrap();
            assert_eq!(
                cached(Some(&root), "e", "s", "f", 100, || Ok(samples(1))).unwrap(),
                samples(1)
            );
            assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
            fs::remove_file(&root).unwrap();
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
            cached(Some(&root), "e", "s", "f", 100, || Ok(samples(2))).unwrap();
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        }
    }
}
