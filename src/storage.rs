use crate::{Error, Result};
use std::{collections::BTreeMap, io::Write, path::PathBuf};
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
        if bytes.len() > self.limit {
            return Ok(());
        }
        self.bytes.remove(key);
        while self.bytes.values().map(Vec::len).sum::<usize>() + bytes.len() > self.limit {
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
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
            Ok(meta) if !meta.is_file() || meta.len() > self.max_entry_bytes => Err(Error::Budget),
            Ok(_) => Ok(Some(std::fs::read(path)?)),
        }
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
