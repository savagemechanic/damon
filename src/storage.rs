//! Crash-safe image transactions. The journal contains independently recoverable,
//! checksummed images, avoiding a second mutation interpreter during replay.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_IMAGE: usize = 16 * 1024 * 1024;
const MAX_JOURNAL: usize = 8 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"DAMONIMG";
const VERSION: u32 = 2;
const HEADER: usize = 32;

pub fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
const CRC_TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};
pub fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ u32::from(byte)) & 255) as usize];
    }
    !crc
}
pub fn frame(generation: u64, payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.len() > MAX_IMAGE {
        return Err(invalid("brain image exceeds 16 MiB limit"));
    }
    let mut out = Vec::with_capacity(HEADER + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&generation.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    let crc = checksum(&out);
    out.splice(28..28, crc.to_le_bytes());
    Ok(out)
}
pub fn unframe(bytes: &[u8]) -> io::Result<(u64, &[u8], usize)> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC {
        return Err(invalid("invalid or truncated image header"));
    }
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    if u32_at(8) != VERSION || u32_at(12) != 0 {
        return Err(invalid("unsupported brain format version or flags"));
    }
    let len = u32_at(24) as usize;
    if len > MAX_IMAGE || len > bytes.len() - HEADER {
        return Err(invalid("invalid or truncated image length"));
    }
    let end = HEADER + len;
    let mut checked = Vec::with_capacity(28 + len);
    checked.extend_from_slice(&bytes[..28]);
    checked.extend_from_slice(&bytes[HEADER..end]);
    if checksum(&checked) != u32_at(28) {
        return Err(invalid("brain checksum mismatch"));
    }
    Ok((
        u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        &bytes[HEADER..end],
        end,
    ))
}
fn side(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("state file exceeds size limit"));
    }
    Ok(bytes)
}
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
fn sync_parent(path: &Path) -> io::Result<()> {
    File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?
    .sync_all()
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = side(path, ".tmp");
    // A stale temporary image is never considered committed.
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    let mut f = private_options().create_new(true).open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&temp, path)?;
    sync_parent(path)
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    _lock: File,
    pub generation: u64,
    pub warnings: Vec<String>,
    journal_len: usize,
    poisoned: bool,
}
impl Store {
    pub fn open(
        path: &Path,
        validate: impl Fn(&[u8]) -> io::Result<()>,
    ) -> io::Result<(Self, Option<Vec<u8>>)> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let path = fs::canonicalize(parent)?.join(
            path.file_name()
                .ok_or_else(|| invalid("missing state filename"))?,
        );
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid("state path must not be a symlink"));
        }
        let lock = private_options()
            .read(true)
            .truncate(false)
            .open(side(&path, ".lock"))?;
        lock.try_lock().map_err(|e| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("brain is already open: {e}"),
            )
        })?;
        let mut store = Self {
            path,
            _lock: lock,
            generation: 0,
            warnings: vec![],
            journal_len: 0,
            poisoned: false,
        };
        let mut best = None;
        let mut existed = false;
        for candidate in [store.path.clone(), side(&store.path, ".prev")] {
            match read_bounded(&candidate, MAX_IMAGE + HEADER) {
                Ok(bytes) => {
                    existed = true;
                    let decoded = if bytes.starts_with(b"DAMON\0\x01\0") {
                        validate(&bytes).map(|()| (0, bytes.as_slice()))
                    } else {
                        unframe(&bytes).and_then(|(g, p, n)| {
                            if n != bytes.len() {
                                return Err(invalid("snapshot has trailing bytes"));
                            }
                            validate(p)?;
                            Ok((g, p))
                        })
                    };
                    match decoded {
                        Ok((g, p)) if best.is_none() || g > store.generation => {
                            store.generation = g;
                            best = Some(p.to_vec());
                        }
                        Ok(_) => {}
                        Err(e) => store.warnings.push(format!(
                            "Ignored invalid snapshot {}: {e}",
                            candidate.display()
                        )),
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        let journal = side(&store.path, ".journal");
        match read_bounded(&journal, MAX_JOURNAL + MAX_IMAGE + HEADER) {
            Ok(bytes) => {
                existed |= !bytes.is_empty();
                let mut pos = 0;
                let mut last = None;
                while pos < bytes.len() {
                    let record = unframe(&bytes[pos..]).and_then(|(g, p, n)| {
                        validate(p)?;
                        if last.is_some_and(|v| g <= v) {
                            return Err(invalid("non-monotonic journal generation"));
                        }
                        Ok((g, p, n))
                    });
                    match record {
                        Ok((g, p, n)) => {
                            last = Some(g);
                            if best.is_none() || g > store.generation {
                                store.generation = g;
                                best = Some(p.to_vec());
                            }
                            pos += n;
                        }
                        Err(e) => {
                            store.warnings.push(format!(
                                "Discarded interrupted/corrupt journal tail at byte {pos}: {e}"
                            ));
                            break;
                        }
                    }
                }
                if pos < bytes.len() {
                    if best.is_none() {
                        return Err(invalid("no valid state before corrupt journal tail"));
                    }
                    // Preserve rejected bytes for explicit diagnosis, bounded to one tail.
                    atomic_write(&side(&store.path, ".rejected"), &bytes[pos..])?;
                    let f = private_options().truncate(false).open(&journal)?;
                    f.set_len(pos as u64)?;
                    f.sync_all()?;
                }
                store.journal_len = pos;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if existed && best.is_none() {
            return Err(invalid(
                "no valid brain snapshot or journal; restore a backup",
            ));
        }
        Ok((store, best))
    }
    pub fn commit(&mut self, payload: &[u8]) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "storage had an uncertain write; reopen before retrying",
            ));
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("generation exhausted"))?;
        let bytes = frame(generation, payload)?;
        let result = (|| {
            let mut f = private_options()
                .append(true)
                .open(side(&self.path, ".journal"))?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            sync_parent(&self.path)?;
            self.generation = generation;
            self.journal_len += bytes.len();
            if self.journal_len >= MAX_JOURNAL || !self.path.exists() {
                self.snapshot(&bytes)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn snapshot(&mut self, bytes: &[u8]) -> io::Result<()> {
        match read_bounded(&self.path, MAX_IMAGE + HEADER) {
            Ok(old) => {
                if unframe(&old).is_ok_and(|(_, _, n)| n == old.len())
                    || old.starts_with(b"DAMON\0\x01\0")
                {
                    atomic_write(&side(&self.path, ".prev"), &old)?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        atomic_write(&self.path, bytes)?;
        let journal = private_options()
            .truncate(true)
            .open(side(&self.path, ".journal"))?;
        journal.sync_all()?;
        sync_parent(&self.path)?;
        self.journal_len = 0;
        Ok(())
    }
    pub fn compact(&mut self, payload: &[u8]) -> io::Result<()> {
        self.commit(payload)?;
        let bytes = frame(self.generation, payload)?;
        let result = self.snapshot(&bytes);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    pub fn export(&self, destination: &Path, payload: &[u8]) -> io::Result<()> {
        // Never overwrite a live brain, journal, or previous export.
        let bytes = frame(self.generation, payload)?;
        let mut f = private_options().create_new(true).open(destination)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        sync_parent(destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_matches_standard_check_vector() {
        assert_eq!(checksum(b"123456789"), 0xcbf43926);
    }
    #[test]
    fn journal_growth_triggers_snapshot_and_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("damon-growth-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brain");
        let (mut s, _) = Store::open(&path, |_| Ok(())).unwrap();
        let payload = vec![42; 1024 * 1024];
        for _ in 0..20 {
            s.commit(&payload).unwrap();
            assert!(s.journal_len < MAX_JOURNAL);
        }
        let generation = s.generation;
        drop(s);
        let (s, recovered) = Store::open(&path, |_| Ok(())).unwrap();
        assert_eq!(s.generation, generation);
        assert_eq!(recovered.unwrap(), payload);
        drop(s);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn header_and_payload_corruption_are_detected() {
        let bytes = frame(7, b"payload").unwrap();
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 1;
            assert!(unframe(&bad).is_err());
        }
        for i in 0..bytes.len() {
            assert!(unframe(&bytes[..i]).is_err());
        }
    }
}
