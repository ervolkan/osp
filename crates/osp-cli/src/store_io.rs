//! Store I/O — kalıcı `AnchorStoreSnapshot` için atomic persistence envelope.
//!
//! İki ayrı sorumluluk (Review 3#2):
//! - **`StoreLock`** (fs4): OS-level advisory exclusive lock, **sabit `.lock` dosyası**
//!   üzerinde. Atomic replace canonical dosyanın inode'unu değiştirebilir; sabit lock
//!   dosyası şart (Review 3 son düzeltme). Process ölünce lock otomatik bırakılır
//!   (stale-lock *state* yok; `.lock` dosyası filesystem'de kalabilir ama bu sorun değil).
//! - **`AtomicStoreWriter`**: aynı dizinde `<rand>.tmp` → serialize → `sync_all` (temp) →
//!   platform-safe atomic rename/replace (Windows `MoveFileEx(MOVEFILE_REPLACE_EXISTING)` veya
//!   POSIX `rename`) → **post-rename** parent dir `sync_all` (POSIX durability).
//!
//! # `PersistedStore` envelope (Review 1#6/R2#4)
//! `revision` domain snapshot'ta değil, persistence envelope'ta. `export_snapshot`
//! revision bilmez; increment `mutate()` transaction'ında (başarılı persistent
//! mutation tam bir kez). Bu, osp-core'u persistence-agnostic tutar.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use osp_core::anchoring::AnchorStoreSnapshot;

use crate::errors::StoreIoError;

/// Kalıcı store envelope — store state + persistence metadata (revision).
///
/// `revision`: monotonic audit/display sayacı. Read-only işlemler artırmaz; başarısız
/// mutation artırmaz; başarılı persistent mutation tam bir kez artırır. Exclusive lock
/// altında increment edildiği için CAS (compare-and-swap) gerekmez — pessimistic lock
/// concurrency'yi sağlar, revision audit/display içindir (Review 2.tur F4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedStore {
    /// Envelope schema version (store-seviye migration; `ConceptGraphSnapshot`'ın
    /// inner `schema_version`'ından ayrı).
    pub store_schema_version: u32,
    /// Monotonic revision — her başarılı persistent mutation +1.
    pub revision: u64,
    /// Store state (graph + iki ledger + audit_seq).
    pub snapshot: AnchorStoreSnapshot,
}

impl PersistedStore {
    /// Mevcut envelope schema version.
    pub const STORE_SCHEMA_VERSION: u32 = 1;

    /// Boş (fresh) envelope — revision 0, boş store. `osp graph init` ilk yazımda.
    pub fn from_snapshot(snapshot: AnchorStoreSnapshot) -> Self {
        Self {
            store_schema_version: Self::STORE_SCHEMA_VERSION,
            revision: 0,
            snapshot,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// StoreLock — fs4 OS-level advisory exclusive lock (sabit .lock dosyası)
// ═══════════════════════════════════════════════════════════════════════════════

/// Exclusive store lock guard. Drop → lock otomatik bırakılır (fs4/OS semantiği).
pub struct StoreLock {
    _file: File,
    /// Lock dosyası path'i (debug/audit için; drop'ta dosya kalabilir — stale değil).
    _lock_path: PathBuf,
}

impl StoreLock {
    /// Canonical store path'inden `.lock` path'i türet: `<store>.lock`.
    fn lock_path_for(store_path: &Path) -> PathBuf {
        let mut p = store_path.as_os_str().to_owned();
        p.push(".lock");
        PathBuf::from(p)
    }

    /// Sabit `.lock` dosyası üzerinde exclusive lock edin. Process ölünce otomatik
    /// bırakılır. Lock dosyası canonical ile aynı dizinde olmalı (cross-volume rename
    /// atomik değil — lock da aynı volume'da).
    pub fn acquire(store_path: &Path) -> Result<Self, StoreIoError> {
        use fs4::fs_std::FileExt;
        let lock_path = Self::lock_path_for(store_path);
        // create(true) — lock dosyası yoksa oluştur (stable .lock file).
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| StoreIoError::LockAcquire {
                path: lock_path.clone(),
                source: e,
            })?;
        // fs4 advisory exclusive lock — process ölünce release.
        file.try_lock_exclusive()
            .map_err(|e| StoreIoError::LockAcquire {
                path: lock_path.clone(),
                source: std::io::Error::other(e),
            })?;
        Ok(Self {
            _file: file,
            _lock_path: lock_path,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AtomicStoreWriter — same-directory tmp → fsync → platform-safe atomic replace
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonical store dosyasını atomik olarak `contents` ile değiştir.
///
/// Akış (Review 3#1):
/// 1. Aynı dizinde `<store>.<rand>.tmp` oluştur (cross-volume rename atomik değil).
/// 2. Yaz + `sync_all` (fsync — içerik diske).
/// 3. Parent directory `sync_all` (directory entry diske).
/// 4. Platform-safe atomic replace:
///    - POSIX: `rename` (existing destination overwrite atomik) + **post-rename** parent dir fsync.
///    - Windows: `MoveFileEx(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
pub fn atomic_replace(store_path: &Path, contents: &[u8]) -> Result<(), StoreIoError> {
    let dir = store_path
        .parent()
        .ok_or_else(|| StoreIoError::InvalidStorePath(store_path.to_path_buf()))?;
    let file_name = store_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| StoreIoError::InvalidStorePath(store_path.to_path_buf()))?;
    // Unique tmp adı: pid + monotonic counter (aynı dizinde collision yok, exclusive lock
    // altında çoğunlukla güvenli ama crash-recovery'de orphan tmp çakışmasını önler).
    let tmp = dir.join(format!(".{}.{}.tmp", file_name, tmp_suffix()));

    // Error-path cleanup guard (Review 2.tur F6): write/sync/rename fail ederse tmp
    // geride kalmasın. Başarılı replace sonrası tmp zaten gitmiştir (no-op remove).
    // Disk-full/crash durumunda best-effort — perfect cleanup guarantee değil.
    let mut cleanup = TmpCleanup::new(tmp.clone());

    // (1) tmp oluştur + yaz.
    {
        let mut file = File::create(&tmp).map_err(|e| StoreIoError::WriteTmp {
            path: tmp.clone(),
            source: e,
        })?;
        file.write_all(contents)
            .map_err(|e| StoreIoError::WriteTmp {
                path: tmp.clone(),
                source: e,
            })?;
        // (2) fsync temp içerik (crash'te yarım dosya yok).
        file.sync_all().map_err(|e| StoreIoError::WriteTmp {
            path: tmp.clone(),
            source: e,
        })?;
    }

    // (3) Atomic replace (rename) — POSIX'te directory entry değişikliği atomik.
    atomic_rename(&tmp, store_path).map_err(|e| StoreIoError::AtomicReplace {
        from: tmp.clone(),
        to: store_path.to_path_buf(),
        source: e,
    })?;
    // Rename başarılı — tmp artık yok, cleanup'ı devre dışı bırak.
    cleanup.disarm();

    // (4) Post-rename parent directory fsync — POSIX durability: rename'in directory
    //     entry değişikliğini crash'e dayanıklı hale getirmek için directory'nin rename'den
    //     SONRA sync edilmesi gerekir (Review P2.2). Windows'ta MOVEFILE_WRITE_THROUGH
    //     bunu zaten sağlar. Best-effort (bazı platformlar/FS'ler desteklemez).
    if let Ok(dir_file) = File::open(dir) {
        let _ = dir_file.sync_all();
    }

    Ok(())
}

/// Unique tmp suffix — process id + monotonic counter, ayraçlı (Review 2.tur F5).
/// `format!("{pid}{c}")` ayracsız collision yaratır: (pid=12,c=34) ve (pid=123,c=4)
/// ikisi de "1234" üretir. Ayraç (`-`) bunu önler.
fn tmp_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", std::process::id(), c)
}

/// Platform-safe atomic rename: Windows'ta `MOVEFILE_REPLACE_EXISTING`, POSIX'te `rename`.
#[cfg(windows)]
fn atomic_rename(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    // std::fs::rename Windows'ta existing destination'da fail edebilir;
    // MoveFileExW(MOVEFILE_REPLACE_EXISTING) atomik replace sağlar.
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    fn to_wide(path: &Path) -> Result<Vec<u16>, std::io::Error> {
        let mut wide: Vec<u16> = OsStr::new(path).encode_wide().collect();
        wide.push(0); // NUL terminator
        Ok(wide)
    }

    let from_w = to_wide(from)?;
    let to_w = to_wide(to)?;
    let ok = unsafe {
        MoveFileExW(
            from_w.as_ptr(),
            to_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_rename(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    // POSIX rename existing destination'ı atomik overwrite eder.
    std::fs::rename(from, to)
}

/// Canonical store dosyasını oku (PersistedStore deserialize).
pub fn read_persisted_store(store_path: &Path) -> Result<PersistedStore, StoreIoError> {
    let data = std::fs::read(store_path).map_err(|e| StoreIoError::Read {
        path: store_path.to_path_buf(),
        source: e,
    })?;
    let persisted: PersistedStore =
        serde_json::from_slice(&data).map_err(|e| StoreIoError::Deserialize {
            path: store_path.to_path_buf(),
            source: e,
        })?;
    if persisted.store_schema_version != PersistedStore::STORE_SCHEMA_VERSION {
        return Err(StoreIoError::UnsupportedStoreSchema {
            expected: PersistedStore::STORE_SCHEMA_VERSION,
            found: persisted.store_schema_version,
        });
    }
    Ok(persisted)
}

/// PersistedStore'u canonical dosyaya atomik yaz (revision envelope içinde).
pub fn write_persisted_store(
    store_path: &Path,
    persisted: &PersistedStore,
) -> Result<(), StoreIoError> {
    let contents =
        serde_json::to_vec_pretty(persisted).map_err(|e| StoreIoError::Serialize { source: e })?;
    atomic_replace(store_path, &contents)
}

/// Error-path tmp cleanup guard (Review 2.tur F6). Drop'ta tmp dosyayı best-effort siler;
/// `disarm()` başarılı replace sonrası no-op'a çevirir. Disk-full vb. perfect guarantee değil.
struct TmpCleanup {
    path: PathBuf,
    armed: bool,
}

impl TmpCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TmpCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osp_core::anchoring::store::InMemoryAnchorStore;
    use tempfile::tempdir;

    fn empty_persisted() -> PersistedStore {
        let store = InMemoryAnchorStore::new();
        PersistedStore::from_snapshot(store.export_snapshot())
    }

    /// Atomic replace: mevcut dosya varken overwrite başarılı (Windows existing-destination).
    #[test]
    fn atomic_replace_overwrites_existing_destination() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("store.json");
        std::fs::write(&path, b"OLD").unwrap();
        atomic_replace(&path, b"NEW").unwrap();
        let back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(back, "NEW");
    }

    /// Atomic replace sonrası tmp dosya kalmıyor.
    #[test]
    fn atomic_replace_leaves_no_tmp_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("store.json");
        atomic_replace(&path, b"DATA").unwrap();
        // tmp dosya kalmamalı.
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        let tmps: Vec<_> = entries
            .iter()
            .filter_map(|e| e.as_ref().ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.ends_with(".tmp"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            tmps.is_empty(),
            "tmp file should be replaced, found {tmps:?}"
        );
    }

    /// PersistedStore round-trip: write → read → identical.
    #[test]
    fn persisted_store_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("store.json");
        let p = empty_persisted();
        write_persisted_store(&path, &p).unwrap();
        let back = read_persisted_store(&path).unwrap();
        assert_eq!(back, p);
    }

    /// Schema mismatch → StoreIoError::UnsupportedStoreSchema.
    #[test]
    fn read_rejects_unsupported_store_schema() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("store.json");
        let mut p = empty_persisted();
        p.store_schema_version = 999;
        write_persisted_store(&path, &p).unwrap();
        let err = read_persisted_store(&path).unwrap_err();
        assert!(matches!(
            err,
            StoreIoError::UnsupportedStoreSchema {
                expected: 1,
                found: 999
            }
        ));
    }

    /// Lock acquire + release: aynı path'de iki acquire ardışık başarılı (drop sonrası).
    #[test]
    fn store_lock_releases_on_drop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("store.json");
        {
            let _lock = StoreLock::acquire(&path).unwrap();
            // lock tutuluyor.
        }
        // drop sonrası tekrar acquire başarılı.
        let _lock2 = StoreLock::acquire(&path).unwrap();
    }
}
