//! Data model, persistence and the force layout.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub type Id = u64;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    Exe,
    Lnk,
    Html,
    Bat,
    Url,
    Dir,
    File,
}

impl Kind {
    pub fn of(path: &Path) -> Kind {
        if path.is_dir() {
            return Kind::Dir;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "exe" => Kind::Exe,
            "lnk" => Kind::Lnk,
            "html" | "htm" => Kind::Html,
            "bat" | "cmd" => Kind::Bat,
            "url" => Kind::Url,
            _ => Kind::File,
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Exe => "EXE",
            Kind::Lnk => "LNK",
            Kind::Html => "HTML",
            Kind::Bat => "BAT",
            Kind::Url => "URL",
            Kind::Dir => "DIR",
            Kind::File => "FILE",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct App {
    pub id: Id,
    pub path: String,
    /// Name pulled from the file itself.
    pub auto_name: String,
    /// User override (right-click > Rename).
    #[serde(default)]
    pub custom_name: Option<String>,
    pub kind: Kind,
    pub pos: [f32; 2],
    #[serde(skip)]
    pub vel: [f32; 2],
    #[serde(skip)]
    pub missing: bool,
}

impl App {
    pub fn name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.auto_name)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cluster {
    pub id: Id,
    pub name: String,
    pub accent: u8,
    pub pos: [f32; 2],
    pub apps: Vec<App>,
    #[serde(skip)]
    pub vel: [f32; 2],
    /// Plate width in world units (set by the UI each frame).
    #[serde(skip)]
    pub plate_w: f32,
}

impl Cluster {
    pub fn orbit_for(&self, n: usize) -> f32 {
        if n == 0 {
            return 0.0;
        }
        (17.0 * n as f32).max(self.plate_w * 0.5 + 62.0).max(130.0)
    }
    pub fn orbit(&self) -> f32 {
        self.orbit_for(self.apps.len())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        View { pan: [0.0, 0.0], zoom: 1.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub version: u32,
    pub next_id: Id,
    pub clusters: Vec<Cluster>,
    #[serde(default)]
    pub view: View,
    /// Ambient float animation on/off.
    #[serde(default = "yes")]
    pub drift: bool,
    /// Last window size in points (restored on start).
    #[serde(default)]
    pub window: Option<[f32; 2]>,
    #[serde(default)]
    pub maximized: bool,
}

fn yes() -> bool {
    true
}

impl Default for Layout {
    fn default() -> Self {
        Layout { version: 1, next_id: 1, clusters: Vec::new(), view: View::default(), drift: true, window: None, maximized: false }
    }
}

// ------------------------------------------------------------------ drift

fn phase(id: Id, k: u64) -> f64 {
    // cheap hash -> [0, TAU)
    let h = (id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ k.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)) >> 40;
    (h as f64 / (1u64 << 24) as f64) * std::f64::consts::TAU
}

/// Slow bob for a cluster (world units). Whole cluster moves together.
/// Time stays f64 so the motion does not get jittery after days of uptime.
pub fn cluster_drift(id: Id, t: f64) -> [f32; 2] {
    [
        ((t * 0.23 + phase(id, 1)).sin() * 11.0 + (t * 0.071 + phase(id, 2)).sin() * 6.0) as f32,
        ((t * 0.19 + phase(id, 3)).sin() * 9.0 + (t * 0.089 + phase(id, 4)).sin() * 5.0) as f32,
    ]
}

/// Extra sway for a bubble around its cluster.
pub fn app_sway(id: Id, t: f64) -> [f32; 2] {
    [((t * 0.41 + phase(id, 5)).sin() * 5.0) as f32, ((t * 0.37 + phase(id, 6)).sin() * 5.0) as f32]
}

/// Shift every node by its drift offset for this frame, recording what was
/// applied so `unapply_drift` can remove exactly that (saved positions never drift).
pub fn apply_drift(layout: &mut Layout, t: f64, applied: &mut std::collections::HashMap<Id, [f32; 2]>) {
    applied.clear();
    for c in &mut layout.clusters {
        let o = cluster_drift(c.id, t);
        c.pos[0] += o[0];
        c.pos[1] += o[1];
        applied.insert(c.id, o);
        for a in &mut c.apps {
            let s = app_sway(a.id, t);
            let d = [o[0] + s[0], o[1] + s[1]];
            a.pos[0] += d[0];
            a.pos[1] += d[1];
            applied.insert(a.id, d);
        }
    }
}

pub fn unapply_drift(layout: &mut Layout, applied: &std::collections::HashMap<Id, [f32; 2]>) {
    for c in &mut layout.clusters {
        if let Some(o) = applied.get(&c.id) {
            c.pos[0] -= o[0];
            c.pos[1] -= o[1];
        }
        for a in &mut c.apps {
            if let Some(o) = applied.get(&a.id) {
                a.pos[0] -= o[0];
                a.pos[1] -= o[1];
            }
        }
    }
}

impl Layout {
    pub fn new_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn add_cluster(&mut self, name: Option<String>, pos: [f32; 2]) -> Id {
        let id = self.new_id();
        let n = self.clusters.len();
        let name = name.unwrap_or_else(|| format!("CLUSTER {:02}", n + 1));
        self.clusters.push(Cluster {
            id,
            name,
            accent: (n % crate::theme::ACCENTS.len()) as u8,
            pos,
            apps: Vec::new(),
            vel: [0.0; 2],
            plate_w: 0.0,
        });
        id
    }

    pub fn cluster_mut(&mut self, id: Id) -> Option<&mut Cluster> {
        self.clusters.iter_mut().find(|c| c.id == id)
    }

    /// Returns (cluster index, app index).
    pub fn find_app(&self, id: Id) -> Option<(usize, usize)> {
        for (ci, c) in self.clusters.iter().enumerate() {
            if let Some(ai) = c.apps.iter().position(|a| a.id == id) {
                return Some((ci, ai));
            }
        }
        None
    }

    /// Repair anything a hand-edited or damaged file could break at runtime:
    /// non-finite numbers, zero zoom, duplicate or stale ids.
    pub fn sanitize(&mut self) {
        fn fix(p: &mut [f32; 2]) {
            for v in p.iter_mut() {
                if !v.is_finite() {
                    *v = 0.0;
                }
                *v = v.clamp(-1.0e6, 1.0e6);
            }
        }
        if !self.view.zoom.is_finite() || self.view.zoom <= 0.0 {
            self.view = View::default();
        }
        self.view.zoom = self.view.zoom.clamp(0.15, 3.0);
        if let Some(w) = self.window {
            if !(w[0].is_finite() && w[1].is_finite()) || w[0] < 320.0 || w[1] < 240.0 || w[0] > 16000.0 || w[1] > 16000.0 {
                self.window = None;
            }
        }
        fix(&mut self.view.pan);
        let mut seen = std::collections::HashSet::new();
        let mut next = self.next_id.max(1);
        for c in &self.clusters {
            next = next.max(c.id + 1);
            for a in &c.apps {
                next = next.max(a.id + 1);
            }
        }
        for c in &mut self.clusters {
            fix(&mut c.pos);
            if !seen.insert(c.id) {
                c.id = next;
                next += 1;
                seen.insert(c.id);
            }
            if c.name.trim().is_empty() {
                c.name = "CLUSTER".into();
            }
            for a in &mut c.apps {
                fix(&mut a.pos);
                if !seen.insert(a.id) {
                    a.id = next;
                    next += 1;
                    seen.insert(a.id);
                }
            }
        }
        self.next_id = next;
    }

    pub fn app_count(&self) -> usize {
        self.clusters.iter().map(|c| c.apps.len()).sum()
    }
}

// ---------------------------------------------------------------- physics


fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// One step of the force layout. Returns the max speed (for settle detection).
/// `pinned` = node currently being dragged by the user (cluster or app id).
pub fn step(layout: &mut Layout, pinned: Option<Id>) -> f32 {
    const DT: f32 = 1.0;
    const DAMP: f32 = 0.82;
    let mut max_v = 0.0f32;

    // Cluster-cluster: soft collision only, so user placement sticks.
    let nc = layout.clusters.len();
    let radii: Vec<f32> = layout
        .clusters
        .iter()
        .map(|c| c.orbit() + 90.0)
        .collect();
    let mut cforce = vec![[0.0f32; 2]; nc];
    for i in 0..nc {
        for j in (i + 1)..nc {
            let d = sub(layout.clusters[i].pos, layout.clusters[j].pos);
            let dist = (d[0] * d[0] + d[1] * d[1]).sqrt().max(0.01);
            let min = radii[i] + radii[j];
            if dist < min {
                let push = (min - dist) * 0.04;
                let (ux, uy) = if dist < 0.1 { (1.0, 0.0) } else { (d[0] / dist, d[1] / dist) };
                cforce[i][0] += ux * push;
                cforce[i][1] += uy * push;
                cforce[j][0] -= ux * push;
                cforce[j][1] -= uy * push;
            }
        }
    }
    for (i, c) in layout.clusters.iter_mut().enumerate() {
        if pinned == Some(c.id) {
            c.vel = [0.0; 2];
            continue;
        }
        c.vel[0] = (c.vel[0] + cforce[i][0] * DT) * DAMP;
        c.vel[1] = (c.vel[1] + cforce[i][1] * DT) * DAMP;
        c.pos[0] += c.vel[0] * DT;
        c.pos[1] += c.vel[1] * DT;
        max_v = max_v.max(c.vel[0].abs() + c.vel[1].abs());
    }

    // Apps: spring to own cluster, repel siblings.
    for c in &mut layout.clusters {
        let n = c.apps.len();
        let rest = c.orbit();
        let center = c.pos;
        let mut f = vec![[0.0f32; 2]; n];
        for i in 0..n {
            let mut d = sub(c.apps[i].pos, center);
            if d[0].abs() + d[1].abs() < 0.5 {
                // Sitting exactly on the center: kick out along an id-derived angle.
                let a = c.apps[i].id as f32 * 2.399_963;
                d = [a.cos(), a.sin()];
            }
            let dist = (d[0] * d[0] + d[1] * d[1]).sqrt().max(0.01);
            let k = (rest - dist) * 0.05;
            f[i][0] += d[0] / dist * k;
            f[i][1] += d[1] / dist * k;
            for j in (i + 1)..n {
                let d = sub(c.apps[i].pos, c.apps[j].pos);
                let d2 = (d[0] * d[0] + d[1] * d[1]).max(1.0);
                let dist = d2.sqrt();
                let rep = (9000.0 / d2).min(12.0);
                let (ux, uy) = if dist < 0.5 { (0.7, 0.7) } else { (d[0] / dist, d[1] / dist) };
                f[i][0] += ux * rep;
                f[i][1] += uy * rep;
                f[j][0] -= ux * rep;
                f[j][1] -= uy * rep;
            }
        }
        for (i, a) in c.apps.iter_mut().enumerate() {
            if pinned == Some(a.id) {
                a.vel = [0.0; 2];
                continue;
            }
            a.vel[0] = (a.vel[0] + f[i][0] * DT) * DAMP;
            a.vel[1] = (a.vel[1] + f[i][1] * DT) * DAMP;
            a.pos[0] += a.vel[0] * DT;
            a.pos[1] += a.vel[1] * DT;
            max_v = max_v.max(a.vel[0].abs() + a.vel[1].abs());
        }
    }
    max_v
}

// ------------------------------------------------------------ persistence

/// Portable mode if `hangar.json` sits next to the exe, otherwise
/// %APPDATA%\Hangar\hangar.json (or ~/.config/hangar on non-Windows).
pub fn data_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("hangar.json");
            if p.exists() {
                return p;
            }
        }
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Hangar").join("hangar.json")
}

/// What happened when the layout file was read at startup.
#[derive(Debug, PartialEq)]
pub enum LoadStatus {
    /// No file yet (first run).
    Fresh,
    Ok,
    /// Main file was damaged; the previous save (.bak) was used instead.
    RecoveredFromBackup,
    /// Main file and backup both unusable; started empty, damaged file kept as .corrupt.
    StartedEmpty,
    /// File exists but could not be read (locked, permissions). Saving is
    /// disabled for this session so the real file is never overwritten.
    Unreadable(String),
}

fn parse(s: &str) -> Option<Layout> {
    let mut l: Layout = serde_json::from_str(s).ok()?;
    l.sanitize();
    Some(l)
}

pub fn load(path: &Path) -> (Layout, LoadStatus) {
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Layout::default(), LoadStatus::Fresh),
        Err(e) => {
            // Not valid UTF-8 counts as damaged; anything else is "hands off".
            if e.kind() != std::io::ErrorKind::InvalidData {
                return (Layout::default(), LoadStatus::Unreadable(e.to_string()));
            }
            String::from("\u{0}")
        }
    };
    if text.trim().is_empty() {
        return (Layout::default(), LoadStatus::Fresh); // portable-mode marker file
    }
    if let Some(l) = parse(&text) {
        return (l, LoadStatus::Ok);
    }
    // Damaged: keep it for inspection, then fall back to the previous save.
    let _ = std::fs::copy(path, path.with_extension("json.corrupt"));
    if let Some(l) = std::fs::read_to_string(path.with_extension("json.bak")).ok().and_then(|s| parse(&s)) {
        return (l, LoadStatus::RecoveredFromBackup);
    }
    (Layout::default(), LoadStatus::StartedEmpty)
}

pub fn save(path: &Path, layout: &Layout) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(layout).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?; // on disk before it replaces the real file
    }
    // Only a file that still parses is allowed to become the backup.
    if std::fs::read_to_string(path).ok().is_some_and(|s| serde_json::from_str::<Layout>(&s).is_ok()) {
        let _ = std::fs::copy(path, path.with_extension("json.bak"));
    }
    std::fs::rename(&tmp, path)
}

/// Missing file, or a shortcut whose absolute local target is gone.
pub fn is_broken(path: &str, kind: Kind) -> bool {
    let p = Path::new(path);
    if !p.exists() {
        return true;
    }
    if kind == Kind::Lnk {
        if let Some(t) = crate::meta::lnk_target(p) {
            let b = t.as_bytes();
            let drive_abs = b.len() > 2 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
            if drive_abs && !Path::new(&t).exists() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hangar-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("hangar.json")
    }

    fn sample() -> Layout {
        let mut l = Layout::default();
        let c = l.add_cluster(Some("TOOLS".into()), [10.0, 20.0]);
        let id = l.new_id();
        l.cluster_mut(c).unwrap().apps.push(App {
            id,
            path: "C:\\x\\a.exe".into(),
            auto_name: "A".into(),
            custom_name: None,
            kind: Kind::Exe,
            pos: [1.0, 2.0],
            vel: [0.0; 2],
            missing: false,
        });
        l
    }

    #[test]
    fn save_load_roundtrip_and_first_run() {
        let p = tmp("rt");
        assert_eq!(load(&p).1, LoadStatus::Fresh);
        save(&p, &sample()).unwrap();
        let (l, st) = load(&p);
        assert_eq!(st, LoadStatus::Ok);
        assert_eq!(l.clusters[0].apps[0].auto_name, "A");
    }

    #[test]
    fn damaged_file_recovers_from_backup_and_backup_survives() {
        let p = tmp("bak");
        save(&p, &sample()).unwrap();
        save(&p, &sample()).unwrap(); // second save creates .bak
        std::fs::write(&p, "{ this is not json").unwrap();
        let (l, st) = load(&p);
        assert_eq!(st, LoadStatus::RecoveredFromBackup);
        assert_eq!(l.clusters.len(), 1);
        assert!(p.with_extension("json.corrupt").exists());
        // Saving now must not replace the good backup with the damaged file.
        save(&p, &l).unwrap();
        let bak = std::fs::read_to_string(p.with_extension("json.bak")).unwrap();
        assert!(serde_json::from_str::<Layout>(&bak).is_ok());
    }

    #[test]
    fn damaged_file_without_backup_starts_empty_but_keeps_copy() {
        let p = tmp("nobak");
        std::fs::write(&p, "garbage").unwrap();
        let (l, st) = load(&p);
        assert_eq!(st, LoadStatus::StartedEmpty);
        assert!(l.clusters.is_empty());
        assert_eq!(std::fs::read_to_string(p.with_extension("json.corrupt")).unwrap(), "garbage");
    }

    #[test]
    fn sanitize_repairs_bad_numbers_and_ids() {
        let mut l = sample();
        l.view.zoom = f32::NAN;
        l.clusters[0].pos = [f32::INFINITY, f32::NAN];
        let dup = l.clusters[0].clone();
        l.clusters.push(dup);
        l.next_id = 0;
        l.sanitize();
        assert!(l.view.zoom.is_finite() && l.view.zoom > 0.0);
        assert!(l.clusters[0].pos.iter().all(|v| v.is_finite()));
        let mut ids: Vec<Id> = l.clusters.iter().flat_map(|c| std::iter::once(c.id).chain(c.apps.iter().map(|a| a.id))).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(l.next_id > *ids.last().unwrap());
    }

    #[test]
    fn physics_stays_finite_with_stacked_nodes() {
        let mut l = sample();
        for _ in 0..30 {
            let id = l.new_id();
            let mut a = l.clusters[0].apps[0].clone();
            a.id = id;
            a.pos = l.clusters[0].pos; // all stacked on the center
            l.clusters[0].apps.push(a);
        }
        let c2 = l.add_cluster(None, l.clusters[0].pos); // cluster on top of cluster
        assert!(c2 > 0);
        let mut v = 0.0;
        for _ in 0..3000 {
            v = step(&mut l, None);
        }
        assert!(v.is_finite() && v < 0.5, "did not settle: {v}");
        for c in &l.clusters {
            assert!(c.pos.iter().all(|x| x.is_finite()));
            for a in &c.apps {
                assert!(a.pos.iter().all(|x| x.is_finite()));
            }
        }
    }
}
