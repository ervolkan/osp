//! Persistence — event-sourcing (milestone snapshot + delta replay).
//!
//! **Faz 2.5:** `SnapshotStore` + `SpaceSnapshot` + `DeltaRecord` + restore/replay.
//!
//! **Event-sourcing pattern (reviewer #2,3):**
//! - Tam snapshot yalnızca milestone'larda (nadir) — `%98 disk tasarrufu`
//! - Her commit'in `Delta`'sı `deltas/` altında (sık)
//! - `restore(t_c)` = en yakın milestone + delta replay → tam `t_c` anına geri yükle
//!
//! **Replay'de pozisyon hesabı YAPILMAZ** — yalnızca graph yapısı (node + edge) geri yüklenir.
//! Pozisyon recomputasyonu engine tarafından post-restore yapılır (lazy, inv #5).

use std::path::{Path, PathBuf};

use crate::bigbang::{apply_delta, Delta};
use crate::space::Space;
use crate::witness::ClaimId;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants + Error
// ═══════════════════════════════════════════════════════════════════════════════

/// bincode format sürümü (reviewer #4). Uyumsuzluk → graceful error.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("persistence I/O hatası: {0}")]
    Io(#[from] std::io::Error),
    #[error("persistence bincode hatası: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("snapshot sürüm uyumsuz: dosya={file} beklenen={expected}")]
    VersionMismatch { file: u32, expected: u32 },
    #[error("milestone bulunamadı (request_t_c={0}); önce save_milestone çağır")]
    NoMilestone(u64),
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceSnapshot + DeltaRecord (bincode serileştirilebilir)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tam Space snapshot — milestone'larda saklanır (nadir).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpaceSnapshot {
    pub version: u32,
    pub t_c: u64,
    pub timestamp_ms: u64,
    pub space: Space,
}

/// Per-commit delta — her commit'te saklanır (sık).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeltaRecord {
    pub version: u32,
    pub t_c: u64,
    pub claim_id: ClaimId,
    pub delta: Delta,
    pub safety_weakened: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// RestoredState (restore çıktısı)
// ═══════════════════════════════════════════════════════════════════════════════

/// Event-sourcing restore sonucu.
#[derive(Debug, Clone)]
pub struct RestoredState {
    pub space: Space,
    pub t_c: u64,
    pub replayed_deltas: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// SnapshotStore (milestone + delta + replay)
// ═══════════════════════════════════════════════════════════════════════════════

/// Event-sourcing persistence manager.
///
/// `milestones/` — tam Space snapshot'ları (`milestone_t{t_c}.bincode`).
/// `deltas/` — per-commit delta'lar (`delta_t{t_c}.bincode`).
pub struct SnapshotStore {
    milestones_dir: PathBuf,
    deltas_dir: PathBuf,
}

impl SnapshotStore {
    /// Yeni store — `base_dir/milestones/` + `base_dir/deltas/` yaratır.
    pub fn new(base_dir: impl AsRef<Path>) -> Result<Self, PersistenceError> {
        let base = base_dir.as_ref();
        let milestones_dir = base.join("milestones");
        let deltas_dir = base.join("deltas");
        std::fs::create_dir_all(&milestones_dir)?;
        std::fs::create_dir_all(&deltas_dir)?;
        Ok(Self {
            milestones_dir,
            deltas_dir,
        })
    }

    // ── Milestone (tam snapshot — nadir) ──────────────────────────────────

    /// Tam Space snapshot kaydet (`milestones/milestone_t{t_c}.bincode`).
    pub fn save_milestone(&self, snapshot: SpaceSnapshot) -> Result<PathBuf, PersistenceError> {
        let path = self
            .milestones_dir
            .join(format!("milestone_t{}.bincode", snapshot.t_c));
        let bytes = bincode::serialize(&snapshot)?;
        std::fs::write(&path, bytes)?;
        tracing::info!(t_c = snapshot.t_c, path = ?path, "milestone kaydedildi");
        Ok(path)
    }

    fn load_milestone_by_t_c(&self, t_c: u64) -> Result<SpaceSnapshot, PersistenceError> {
        let path = self
            .milestones_dir
            .join(format!("milestone_t{}.bincode", t_c));
        let bytes = std::fs::read(&path)?;
        let snapshot: SpaceSnapshot = bincode::deserialize(&bytes)?;
        self.check_version(snapshot.version)?;
        Ok(snapshot)
    }

    /// Mevcut milestone'ların t_c değerlerini (sorted).
    pub fn list_milestones(&self) -> Result<Vec<u64>, PersistenceError> {
        let mut t_cs: Vec<u64> = Vec::new();
        for entry in std::fs::read_dir(&self.milestones_dir)? {
            let entry = entry?;
            if let Some(t_c) =
                parse_t_c_from_filename(&entry.file_name().to_string_lossy(), "milestone_t")
            {
                t_cs.push(t_c);
            }
        }
        t_cs.sort_unstable();
        Ok(t_cs)
    }

    /// `request_t_c`'ye en yakın `t_c ≤ request_t_c` milestone bul.
    fn find_nearest_milestone(&self, request_t_c: u64) -> Option<u64> {
        self.list_milestones()
            .ok()?
            .into_iter()
            .filter(|&t| t <= request_t_c)
            .max()
    }

    // ── Delta (per-commit — sık) ───────────────────────────────────────────

    /// Per-commit delta kaydet (`deltas/delta_t{t_c}.bincode`).
    pub fn save_delta(&self, record: DeltaRecord) -> Result<PathBuf, PersistenceError> {
        let path = self
            .deltas_dir
            .join(format!("delta_t{}.bincode", record.t_c));
        let bytes = bincode::serialize(&record)?;
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    fn load_delta_by_t_c(&self, t_c: u64) -> Result<DeltaRecord, PersistenceError> {
        let path = self.deltas_dir.join(format!("delta_t{}.bincode", t_c));
        let bytes = std::fs::read(&path)?;
        let record: DeltaRecord = bincode::deserialize(&bytes)?;
        self.check_version(record.version)?;
        Ok(record)
    }

    /// `(from_exclusive, to_inclusive]` aralığındaki delta t_c'leri (sorted).
    pub(crate) fn list_deltas_in_range(
        &self,
        from_exclusive: u64,
        to_inclusive: u64,
    ) -> Result<Vec<u64>, PersistenceError> {
        let mut t_cs: Vec<u64> = Vec::new();
        for entry in std::fs::read_dir(&self.deltas_dir)? {
            let entry = entry?;
            if let Some(t_c) =
                parse_t_c_from_filename(&entry.file_name().to_string_lossy(), "delta_t")
            {
                if t_c > from_exclusive && t_c <= to_inclusive {
                    t_cs.push(t_c);
                }
            }
        }
        t_cs.sort_unstable();
        Ok(t_cs)
    }

    // ── Restore (event-sourcing: milestone + delta replay) ─────────────────

    /// `request_t_c` anına geri yükle (reviewer #3).
    ///
    /// 1. `t_c ≤ request_t_c` olan en büyük milestone'u bul + yükle
    /// 2. `(milestone_t_c, request_t_c]` aralığındaki delta'ları sırayla replay
    /// 3. → `RestoredState { space @ request_t_c, replayed_deltas }`
    ///
    /// **Replay'de pozisyon hesabı YAPILMAZ** — graph yapısı (node + edge) geri yüklenir.
    /// Engine post-restore'da `CosineDeviation` ile pozisyon recomputasyonu yapar (lazy).
    pub fn restore(&self, request_t_c: u64) -> Result<RestoredState, PersistenceError> {
        let milestone_t_c = self.find_nearest_milestone(request_t_c);

        let mut space = if let Some(tc) = milestone_t_c {
            let snapshot = self.load_milestone_by_t_c(tc)?;
            snapshot.space
        } else {
            // Milestone yok → boş space'den başla (t_c=0)
            Space::new()
        };

        let from_exclusive = milestone_t_c.unwrap_or(0);
        let delta_t_cs = self.list_deltas_in_range(from_exclusive, request_t_c)?;

        let mut replayed = 0;
        for delta_t_c in delta_t_cs {
            let record = self.load_delta_by_t_c(delta_t_c)?;
            // Replay: graph yapısını geri yükle (pozisyon değil)
            let _repositioned = apply_delta(&mut space, &record.delta);
            replayed += 1;
        }

        tracing::info!(
            request_t_c,
            milestone_t_c = milestone_t_c.unwrap_or(0),
            replayed,
            "restore tamamlandı"
        );

        Ok(RestoredState {
            space,
            t_c: request_t_c,
            replayed_deltas: replayed,
        })
    }

    // ── Version check ─────────────────────────────────────────────────────

    fn check_version(&self, file_version: u32) -> Result<(), PersistenceError> {
        if file_version != SNAPSHOT_FORMAT_VERSION {
            Err(PersistenceError::VersionMismatch {
                file: file_version,
                expected: SNAPSHOT_FORMAT_VERSION,
            })
        } else {
            Ok(())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper: filename → t_c parse
// ═══════════════════════════════════════════════════════════════════════════════

/// `milestone_t100.bincode` → `Some(100)`. `delta_t42.bincode` → `Some(42)`.
fn parse_t_c_from_filename(filename: &str, prefix: &str) -> Option<u64> {
    let after_prefix = filename.strip_prefix(prefix)?;
    let num_str: String = after_prefix
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse().ok()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::{Edge, EdgeKind, Node, NodeKind};
    use crate::witness::ClaimId;

    fn mod_node(id: u64) -> Node {
        Node {
            id,
            kind: NodeKind::Module,
            ..Default::default()
        }
    }

    fn edge(from: u64, to: u64) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Imports,
            ..Default::default()
        }
    }

    fn snapshot(t_c: u64, node_ids: &[u64]) -> SpaceSnapshot {
        let mut space = Space::new();
        for &id in node_ids {
            space.insert_node(mod_node(id));
        }
        SpaceSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            t_c,
            timestamp_ms: 0,
            space,
        }
    }

    fn delta_record(t_c: u64, new_node_ids: &[u64], new_edges: &[(u64, u64)]) -> DeltaRecord {
        DeltaRecord {
            version: SNAPSHOT_FORMAT_VERSION,
            t_c,
            claim_id: t_c as ClaimId,
            delta: Delta {
                new_nodes: new_node_ids.iter().map(|&id| mod_node(id)).collect(),
                new_edges: new_edges.iter().map(|&(f, t)| edge(f, t)).collect(),
                removed_edges: vec![], // G2c-2
                repositioned: vec![],
            },
            safety_weakened: false,
        }
    }

    // --- save/load roundtrip ---

    #[test]
    fn save_load_milestone_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();
        let snap = snapshot(10, &[1, 2, 3]);
        store.save_milestone(snap.clone()).unwrap();

        // Load via restore(t_c=10) → milestone only, no deltas
        let restored = store.restore(10).unwrap();
        assert_eq!(restored.t_c, 10);
        assert_eq!(restored.space.node_count(), 3);
        assert_eq!(restored.replayed_deltas, 0);
    }

    #[test]
    fn save_load_delta_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();
        // Milestone at t_c=0 (empty)
        store.save_milestone(snapshot(0, &[])).unwrap();
        // Delta at t_c=1 adds node 10
        store.save_delta(delta_record(1, &[10], &[])).unwrap();

        let restored = store.restore(1).unwrap();
        assert_eq!(restored.space.node_count(), 1);
        assert!(restored.space.nodes.contains_key(&10));
        assert_eq!(restored.replayed_deltas, 1);
    }

    // --- event-sourcing: milestone + delta replay ---

    #[test]
    fn restore_replays_deltas_from_milestone() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();

        // Milestone at t_c=5: nodes {1,2}
        store.save_milestone(snapshot(5, &[1, 2])).unwrap();
        // Deltas t_c=6,7,8: add nodes 3,4,5
        store.save_delta(delta_record(6, &[3], &[])).unwrap();
        store.save_delta(delta_record(7, &[4], &[])).unwrap();
        store.save_delta(delta_record(8, &[5], &[])).unwrap();

        let restored = store.restore(8).unwrap();
        assert_eq!(restored.t_c, 8);
        assert_eq!(restored.space.node_count(), 5); // {1,2} + {3,4,5}
        assert_eq!(restored.replayed_deltas, 3);
        for id in 1..=5 {
            assert!(
                restored.space.nodes.contains_key(&id),
                "node {} mevcut olmalı",
                id
            );
        }
    }

    #[test]
    fn restore_mid_range_stops_at_request_t_c() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();

        store.save_milestone(snapshot(0, &[])).unwrap();
        store.save_delta(delta_record(1, &[10], &[])).unwrap();
        store.save_delta(delta_record(2, &[11], &[])).unwrap();
        store.save_delta(delta_record(3, &[12], &[])).unwrap();

        // Restore to t_c=2 → only deltas 1 and 2 replayed (not 3)
        let restored = store.restore(2).unwrap();
        assert_eq!(restored.space.node_count(), 2); // {10, 11}
        assert_eq!(restored.replayed_deltas, 2);
        assert!(restored.space.nodes.contains_key(&10));
        assert!(restored.space.nodes.contains_key(&11));
        assert!(!restored.space.nodes.contains_key(&12)); // t_c=3 not replayed
    }

    #[test]
    fn restore_edges_preserved_through_replay() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();

        store.save_milestone(snapshot(0, &[])).unwrap();
        store
            .save_delta(delta_record(1, &[10, 11], &[(10, 11)]))
            .unwrap();

        let restored = store.restore(1).unwrap();
        assert_eq!(restored.space.edge_count(), 1);
        assert_eq!(restored.space.edges[0].from, 10);
        assert_eq!(restored.space.edges[0].to, 11);
    }

    // --- edge cases ---

    #[test]
    fn restore_empty_store_returns_empty_space() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();
        // Hiç milestone/delta yok → boş space
        let restored = store.restore(0).unwrap();
        assert_eq!(restored.space.node_count(), 0);
        assert_eq!(restored.replayed_deltas, 0);
    }

    #[test]
    fn restore_no_matching_milestone_starts_from_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();
        // Milestone at t_c=100, request t_c=50 → no milestone ≤ 50 → empty
        store.save_milestone(snapshot(100, &[1])).unwrap();
        let restored = store.restore(50).unwrap();
        assert_eq!(restored.space.node_count(), 0);
    }

    #[test]
    fn restore_uses_nearest_milestone_not_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();
        store.save_milestone(snapshot(10, &[1, 2])).unwrap();
        store
            .save_milestone(snapshot(50, &[1, 2, 3, 4, 5]))
            .unwrap();

        // Restore t_c=30 → nearest milestone ≤ 30 is t_c=10 (not 50)
        let restored = store.restore(30).unwrap();
        assert_eq!(restored.space.node_count(), 2); // {1,2} from t_c=10
    }

    // --- version check (reviewer #4) ---

    #[test]
    fn version_mismatch_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();

        // Manually write a snapshot with wrong version
        let bad_snapshot = SpaceSnapshot {
            version: 999, // wrong!
            t_c: 1,
            timestamp_ms: 0,
            space: Space::new(),
        };
        let bytes = bincode::serialize(&bad_snapshot).unwrap();
        let path = store.milestones_dir.join("milestone_t1.bincode");
        std::fs::write(&path, bytes).unwrap();

        let result = store.restore(1);
        assert!(matches!(
            result,
            Err(PersistenceError::VersionMismatch {
                file: 999,
                expected: 1
            })
        ));
    }

    // --- filename parsing ---

    #[test]
    fn parse_t_c_from_filename_works() {
        assert_eq!(
            parse_t_c_from_filename("milestone_t42.bincode", "milestone_t"),
            Some(42)
        );
        assert_eq!(
            parse_t_c_from_filename("delta_t100.bincode", "delta_t"),
            Some(100)
        );
        assert_eq!(parse_t_c_from_filename("other.txt", "milestone_t"), None);
        assert_eq!(
            parse_t_c_from_filename("milestone_t.bincode", "milestone_t"),
            None
        ); // no digits
    }

    // --- list helpers ---

    #[test]
    fn list_milestones_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();
        store.save_milestone(snapshot(50, &[])).unwrap();
        store.save_milestone(snapshot(10, &[])).unwrap();
        store.save_milestone(snapshot(30, &[])).unwrap();

        let t_cs = store.list_milestones().unwrap();
        assert_eq!(t_cs, vec![10, 30, 50]);
    }

    #[test]
    fn delta_record_serializes_correctly() {
        let record = delta_record(42, &[1, 2], &[(1, 2)]);
        let bytes = bincode::serialize(&record).unwrap();
        let restored: DeltaRecord = bincode::deserialize(&bytes).unwrap();
        assert_eq!(restored.t_c, 42);
        assert_eq!(restored.delta.new_nodes.len(), 2);
        assert_eq!(restored.delta.new_edges.len(), 1);
    }

    // --- disk efficiency (concept test) ---

    #[test]
    fn delta_files_smaller_than_milestone() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(tmp.path()).unwrap();

        // 1000-node milestone
        let mut big_space = Space::new();
        for i in 0..1000 {
            big_space.insert_node(mod_node(i));
        }
        let snap = SpaceSnapshot {
            version: 1,
            t_c: 0,
            timestamp_ms: 0,
            space: big_space,
        };
        let milestone_path = store.save_milestone(snap).unwrap();
        let milestone_size = std::fs::metadata(&milestone_path).unwrap().len();

        // 2-node delta
        let delta = delta_record(1, &[1000, 1001], &[(1000, 1001)]);
        let delta_path = store.save_delta(delta).unwrap();
        let delta_size = std::fs::metadata(&delta_path).unwrap().len();

        assert!(
            delta_size < milestone_size,
            "delta ({}) < milestone ({}) — event-sourcing disk tasarrufu",
            delta_size,
            milestone_size
        );
    }
}
