use crate::{Error, Result};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

// Open the leaf through a non-following handle. A symlink_metadata/path-read
// sequence has a TOCTOU window where a cache entry can be swapped for a link.
#[cfg(unix)]
fn open_cache_leaf(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn open_cache_leaf(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_FLAG_OPEN_REPARSE_POINT: open a leaf reparse point, not its target.
    // The subsequent handle metadata must be a regular file.
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_cache_leaf(_path: &Path) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "secure cache file opening is not supported on this platform",
    ))
}
pub trait ByteStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &str, bytes: &[u8]) -> Result<()>;
}
pub struct MemoryStore {
    bytes: BTreeMap<String, Vec<u8>>,
    limit: usize,
}
impl MemoryStore {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: BTreeMap::new(),
            limit,
        }
    }
}
impl ByteStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.bytes.get(key).cloned())
    }
    fn put(&mut self, key: &str, bytes: &[u8]) -> Result<()> {
        // Subtract before comparing: occupied + incoming could overflow usize
        // at the largest caller-configured cache limits.
        let Some(remaining_capacity) = self.limit.checked_sub(bytes.len()) else {
            return Ok(());
        };
        self.bytes.remove(key);
        while self.bytes.values().map(Vec::len).sum::<usize>() > remaining_capacity {
            self.bytes.pop_first();
        }
        self.bytes.insert(key.into(), bytes.to_vec());
        Ok(())
    }
}
/// Private per-product directory. Digest keys prevent release names from becoming paths.
pub struct FileStore {
    directory: PathBuf,
    max_entry_bytes: u64,
}
impl FileStore {
    pub fn new(directory: PathBuf, max_entry_bytes: u64) -> Result<Self> {
        if max_entry_bytes == 0 {
            return Err(Error::Budget);
        }
        std::fs::create_dir_all(&directory)?;
        Ok(Self {
            directory,
            max_entry_bytes,
        })
    }
    fn path(&self, key: &str) -> Result<PathBuf> {
        if key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Integrity);
        }
        Ok(self.directory.join(key))
    }
}
impl ByteStore for FileStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path(key)?;
        let file = match open_cache_leaf(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(file) => file,
        };
        // Inspect the opened handle, not the pathname: path checks can race a
        // replacement. Bound reads even if another actor grows the file later.
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > self.max_entry_bytes {
            return Err(Error::Budget);
        }
        let mut bytes = Vec::new();
        file.take(self.max_entry_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > self.max_entry_bytes {
            return Err(Error::Budget);
        }
        Ok(Some(bytes))
    }
    fn put(&mut self, key: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > self.max_entry_bytes {
            return Err(Error::Budget);
        }
        let path = self.path(key)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.directory)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| Error::Io(e.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod bounded_memory_store_tests {
    use super::{ByteStore, MemoryStore};

    #[test]
    fn cache_eviction_never_exceeds_capacity_or_returns_old_keys() {
        let mut cache = MemoryStore::new(5);
        cache.put("a", b"123").expect("cache write");
        cache.put("b", b"45").expect("cache write");
        cache.put("c", b"6789").expect("cache write");
        assert_eq!(cache.get("a").expect("cache read"), None);
        assert_eq!(cache.get("b").expect("cache read"), None);
        assert_eq!(cache.get("c").expect("cache read"), Some(b"6789".to_vec()));

        cache.put("c", b"xy").expect("overwrite");
        cache.put("d", b"rst").expect("cache write");
        assert_eq!(cache.get("c").expect("cache read"), Some(b"xy".to_vec()));
        assert_eq!(cache.get("d").expect("cache read"), Some(b"rst".to_vec()));
        cache
            .put("huge", b"123456")
            .expect("oversized is uncacheable");
        assert_eq!(cache.get("huge").expect("cache read"), None);
        assert_eq!(cache.get("c").expect("cache read"), Some(b"xy".to_vec()));
    }

    #[test]
    fn maximum_usize_capacity_accepts_small_values_without_overflow() {
        let mut cache = MemoryStore::new(usize::MAX);
        cache.put("one", b"x").expect("cache write");
        cache.put("two", b"yz").expect("cache write");
        assert_eq!(cache.get("one").expect("cache read"), Some(b"x".to_vec()));
        assert_eq!(cache.get("two").expect("cache read"), Some(b"yz".to_vec()));
    }
}
