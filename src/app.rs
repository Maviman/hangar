//! Canvas, interaction and chrome.

use crate::meta;
use crate::model::{self, Id, Kind, Layout};
use crate::platform;
use crate::theme::*;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Key, Painter, PointerButton, Pos2, Rect, Sense, Shape,
    Stroke, TextureHandle, Vec2,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Cluster(Id),
    App(Id),
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Pan,
    Cluster(Id),
    App(Id),
}

enum Payload {
    Files(Vec<PathBuf>),
    Move(Id),
}

/// Searchable "which cluster?" popup shown after a drop on open space.
struct Picker {
    payload: Payload,
    world: [f32; 2],
    screen: Pos2,
    query: String,
    sel: usize,
    preselect: Option<Id>,
    focus_frames: u8,
}

enum PickChoice {
    Existing(Id),
    New(String),
}

struct Rename {
    target: Hit,
    text: String,
    focus_frames: u8,
}

struct Toast {
    text: String,
    color: Color32,
    until: f64,
}

#[derive(Clone, Copy)]
struct Xf {
    origin: Pos2,
    zoom: f32,
}

impl Xf {
    fn s(&self, w: [f32; 2]) -> Pos2 {
        self.origin + Vec2::new(w[0], w[1]) * self.zoom
    }
    fn w(&self, s: Pos2) -> [f32; 2] {
        let v = (s - self.origin) / self.zoom;
        [v.x, v.y]
    }
}

/// Results coming back from background threads.
enum Res {
    Icon(String, Option<platform::Rgba>),
    Broken(Id, bool),
    Picked(Id, Option<PathBuf>),
}

enum Req {
    Icon(String),
    Check(Id, String, Kind),
}

fn inbox_path() -> PathBuf {
    model::data_path().with_file_name("inbox.txt")
}

/// Called by a second copy of the exe: hand its files to the running one.
pub fn write_inbox(paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    use std::io::Write;
    let p = inbox_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        for x in paths {
            let _ = writeln!(f, "{}", x.display());
        }
    }
}

pub struct HangarApp {
    layout: Layout,
    data_path: PathBuf,
    dirty_at: Option<f64>,
    icons: HashMap<String, Option<TextureHandle>>,
    drag: Option<Drag>,
    ctx_target: Option<Hit>,
    ctx_world: [f32; 2],
    rename: Option<Rename>,
    picker: Option<Picker>,
    search: String,
    focus_search: bool,
    toasts: Vec<Toast>,
    flashes: Vec<(Id, f64)>,
    last_launch: HashMap<Id, f64>,
    settled: bool,
    drift_applied: HashMap<Id, [f32; 2]>,
    show_help: bool,
    /// Cluster plate rects in world space, rebuilt every frame.
    plates: Vec<(Id, Rect)>,
    first_frame: bool,
    pending: Option<Vec<PathBuf>>,
    ctx: egui::Context,
    req_tx: std::sync::mpsc::Sender<Req>,
    res_tx: std::sync::mpsc::Sender<Res>,
    res_rx: std::sync::mpsc::Receiver<Res>,
    dialog_open: bool,
    undo: Vec<Layout>,
    /// Set when the layout file exists but could not be read: never overwrite it.
    save_blocked: Option<String>,
    save_failed: bool,
    was_focused: bool,
    inbox_checked: f64,
    startup_note: Option<(String, Color32)>,
    selftest_frames: Option<u32>,
}

impl HangarApp {
    pub fn new(ctx: egui::Context, args: Vec<PathBuf>, selftest: bool) -> Self {
        let data_path = model::data_path();
        let (layout, status) = model::load(&data_path);
        let mut save_blocked = None;
        let startup_note = match status {
            model::LoadStatus::Fresh | model::LoadStatus::Ok => None,
            model::LoadStatus::RecoveredFromBackup => {
                Some(("LAYOUT FILE WAS DAMAGED // RESTORED PREVIOUS SAVE".to_string(), EMBER))
            }
            model::LoadStatus::StartedEmpty => {
                Some(("LAYOUT FILE WAS DAMAGED // KEPT AS HANGAR.JSON.CORRUPT".to_string(), EMBER))
            }
            model::LoadStatus::Unreadable(e) => {
                save_blocked = Some(e);
                Some(("LAYOUT FILE COULD NOT BE READ // CHANGES WILL NOT BE SAVED".to_string(), EMBER))
            }
        };
        let pending = if args.is_empty() || selftest { None } else { Some(args) };
        if selftest {
            save_blocked = Some("selftest".into()); // a self-test never writes user data
        }

        // Worker: shell icon lookups and "does this still exist" checks can
        // block for seconds on network or sleeping drives, so keep them off the UI thread.
        let (req_tx, req_rx) = std::sync::mpsc::channel::<Req>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<Res>();
        {
            let (tx, ctx) = (res_tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                platform::thread_init();
                while let Ok(req) = req_rx.recv() {
                    let res = match req {
                        Req::Icon(path) => {
                            let img = platform::file_icon(Path::new(&path));
                            Res::Icon(path, img)
                        }
                        Req::Check(id, path, kind) => Res::Broken(id, model::is_broken(&path, kind)),
                    };
                    if tx.send(res).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        for c in &layout.clusters {
            for a in &c.apps {
                let _ = req_tx.send(Req::Check(a.id, a.path.clone(), a.kind));
            }
        }
        HangarApp {
            layout,
            data_path,
            dirty_at: None,
            icons: HashMap::new(),
            drag: None,
            ctx_target: None,
            ctx_world: [0.0; 2],
            rename: None,
            picker: None,
            search: String::new(),
            focus_search: false,
            toasts: Vec::new(),
            flashes: Vec::new(),
            last_launch: HashMap::new(),
            settled: false,
            drift_applied: HashMap::new(),
            show_help: false,
            plates: Vec::new(),
            first_frame: true,
            pending,
            ctx,
            req_tx,
            res_tx,
            res_rx,
            dialog_open: false,
            undo: Vec::new(),
            save_blocked,
            save_failed: false,
            was_focused: true,
            inbox_checked: 0.0,
            startup_note,
            selftest_frames: if selftest { Some(0) } else { None },
        }
    }

    fn touch(&mut self, now: f64) {
        self.dirty_at.get_or_insert(now);
        self.settled = false;
    }

    fn toast(&mut self, now: f64, text: impl Into<String>, color: Color32) {
        self.toasts.push(Toast { text: text.into(), color, until: now + 2.6 });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn save_now(&mut self, now: f64) {
        self.dirty_at = None;
        if self.save_blocked.is_some() {
            return;
        }
        match model::save(&self.data_path, &self.layout) {
            Ok(()) => self.save_failed = false,
            Err(e) => {
                if !self.save_failed {
                    self.toasts.push(Toast { text: format!("SAVE FAILED // {e} // RETRYING"), color: EMBER, until: now + 8.0 });
                }
                self.save_failed = true;
                // Stay dirty and retry in ~5 s (the autosave fires 1 s after dirty_at).
                self.dirty_at = Some(now + 4.0);
            }
        }
    }

    /// Remember the layout before a change so Ctrl+Z can bring it back.
    /// Drift offsets applied for this frame are stripped from the copy.
    fn snapshot(&mut self) {
        let mut copy = self.layout.clone();
        model::unapply_drift(&mut copy, &self.drift_applied);
        self.undo.push(copy);
        if self.undo.len() > 30 {
            self.undo.remove(0);
        }
    }

    fn undo(&mut self, now: f64) {
        let Some(prev) = self.undo.pop() else {
            self.toast(now, "NOTHING TO UNDO", ASH);
            return;
        };
        let view = self.layout.view.clone();
        let (window, maximized, drift) = (self.layout.window, self.layout.maximized, self.layout.drift);
        self.layout = prev;
        self.layout.view = view;
        self.layout.window = window;
        self.layout.maximized = maximized;
        self.layout.drift = drift;
        self.drift_applied.clear(); // restored positions carry no drift offset
        self.rename = None;
        self.picker = None;
        self.drag = None;
        for c in &self.layout.clusters {
            for a in &c.apps {
                let _ = self.req_tx.send(Req::Check(a.id, a.path.clone(), a.kind));
            }
        }
        let left = self.undo.len();
        self.toast(now, format!("UNDO // {left} MORE"), BONE);
        self.touch(now);
    }

    /// Apply finished background work and files handed over by a second copy.
    fn pump(&mut self, now: f64, focused: bool) {
        while let Ok(res) = self.res_rx.try_recv() {
            match res {
                Res::Icon(path, img) => {
                    let tex = img.map(|img| {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &img.px);
                        self.ctx.load_texture(&path, ci, egui::TextureOptions::LINEAR)
                    });
                    self.icons.insert(path, tex);
                }
                Res::Broken(id, broken) => {
                    if let Some((ci, ai)) = self.layout.find_app(id) {
                        self.layout.clusters[ci].apps[ai].missing = broken;
                    }
                }
                Res::Picked(id, picked) => {
                    self.dialog_open = false;
                    if let Some(new) = picked {
                        self.apply_relocate(id, new, now);
                    }
                }
            }
        }
        // inbox.txt: checked when the window gains focus, and at most every 2 s otherwise.
        let gained = focused && !self.was_focused;
        self.was_focused = focused;
        if gained || now - self.inbox_checked > 2.0 {
            self.inbox_checked = now;
            let p = inbox_path();
            if let Ok(text) = std::fs::read_to_string(&p) {
                let _ = std::fs::remove_file(&p);
                let paths: Vec<PathBuf> = text.lines().map(PathBuf::from).filter(|p| p.exists()).collect();
                if !paths.is_empty() {
                    self.dock_to_inbox(paths, now);
                }
            }
        }
    }

    fn dock_to_inbox(&mut self, paths: Vec<PathBuf>, now: f64) {
        let inbox = self.layout.clusters.iter().find(|c| c.name.eq_ignore_ascii_case("INBOX")).map(|c| c.id);
        let id = inbox.unwrap_or_else(|| self.layout.add_cluster(Some("INBOX".into()), [0.0, 0.0]));
        self.add_paths_to(id, paths, now);
    }

    // ------------------------------------------------------------ geometry

    fn xf(&self, canvas: Rect) -> Xf {
        let v = &self.layout.view;
        Xf { origin: canvas.center() + Vec2::new(v.pan[0], v.pan[1]), zoom: v.zoom }
    }

    fn rebuild_plates(&mut self, painter: &Painter) {
        self.plates.clear();
        for c in &self.layout.clusters {
            let g = painter.layout_job(spaced(&c.name, display(20.0), VOID, 1.0));
            let w = g.size().x + 52.0;
            let r = Rect::from_center_size(Pos2::new(c.pos[0], c.pos[1]), Vec2::new(w.max(96.0), PLATE_H));
            self.plates.push((c.id, r));
        }
        for (c, (_, r)) in self.layout.clusters.iter_mut().zip(&self.plates) {
            c.plate_w = r.width();
        }
    }

    fn hit(&self, w: [f32; 2], zoom: f32, skip: Option<Hit>) -> Option<Hit> {
        let p = Pos2::new(w[0], w[1]);
        let slop = 4.0 / zoom;
        for (id, r) in self.plates.iter().rev() {
            if Some(Hit::Cluster(*id)) != skip && r.expand(slop).contains(p) {
                return Some(Hit::Cluster(*id));
            }
        }
        for c in self.layout.clusters.iter().rev() {
            for a in c.apps.iter().rev() {
                if Some(Hit::App(a.id)) == skip {
                    continue;
                }
                let d = Vec2::new(a.pos[0] - w[0], a.pos[1] - w[1]).length();
                if d <= BUBBLE_R + slop {
                    return Some(Hit::App(a.id));
                }
            }
        }
        None
    }

    /// Which cluster would receive something dropped at `w`.
    fn drop_cluster(&self, w: [f32; 2], zoom: f32, skip: Option<Hit>) -> Option<Id> {
        match self.hit(w, zoom, skip) {
            Some(Hit::Cluster(id)) => return Some(id),
            Some(Hit::App(id)) => {
                return self.layout.find_app(id).map(|(ci, _)| self.layout.clusters[ci].id)
            }
            None => {}
        }
        let mut best: Option<(f32, Id)> = None;
        for c in &self.layout.clusters {
            let d = Vec2::new(c.pos[0] - w[0], c.pos[1] - w[1]).length();
            let reach = c.orbit().max(80.0) + BUBBLE_R + 30.0;
            if d < reach && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c.id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn fit_view(&mut self, canvas: Rect) {
        let mut bb = Rect::NOTHING;
        for c in &self.layout.clusters {
            bb.extend_with(Pos2::new(c.pos[0], c.pos[1]));
            for a in &c.apps {
                bb.extend_with(Pos2::new(a.pos[0], a.pos[1]));
            }
        }
        if !bb.is_positive() && bb.min.x > bb.max.x {
            self.layout.view = model::View::default();
            return;
        }
        let bb = bb.expand(120.0);
        let zoom = (canvas.width() / bb.width()).min(canvas.height() / bb.height()).clamp(0.15, 1.6);
        let c = bb.center();
        self.layout.view = model::View { pan: [-c.x * zoom, -c.y * zoom], zoom };
    }

    // ------------------------------------------------------------- actions

    fn add_paths_to(&mut self, cid: Id, paths: Vec<PathBuf>, now: f64) {
        self.snapshot();
        let mut added = 0;
        let mut last = String::new();
        for p in paths {
            let path_s = p.to_string_lossy().into_owned();
            let dup = self
                .layout
                .clusters
                .iter()
                .find(|c| c.id == cid)
                .is_some_and(|c| c.apps.iter().any(|a| a.path.eq_ignore_ascii_case(&path_s)));
            if dup {
                self.toast(now, format!("ALREADY DOCKED // {}", meta::stem(&p)), ASH);
                continue;
            }
            let kind = Kind::of(&p);
            let name = meta::display_name(&p, kind);
            let id = self.layout.new_id();
            let Some(c) = self.layout.cluster_mut(cid) else { return };
            let k = c.apps.len() as f32;
            let ang = k * 2.399_963; // golden angle
            let r = c.orbit_for(c.apps.len() + 1);
            let pos = [c.pos[0] + ang.cos() * r, c.pos[1] + ang.sin() * r];
            last = name.clone();
            c.apps.push(model::App {
                id,
                path: path_s,
                auto_name: name,
                custom_name: None,
                kind,
                pos,
                vel: [0.0; 2],
                missing: false,
            });
            added += 1;
        }
        if added == 1 {
            self.toast(now, format!("DOCKED // {last}"), ACID);
        } else if added > 1 {
            self.toast(now, format!("DOCKED // {added} NODES"), ACID);
        }
        self.touch(now);
    }

    fn launch(&mut self, id: Id, now: f64) {
        if self.last_launch.get(&id).is_some_and(|t| now - t < 0.8) {
            return; // swallow the second click of a double-click
        }
        let Some((ci, ai)) = self.layout.find_app(id) else { return };
        let a = &mut self.layout.clusters[ci].apps[ai];
        let path = PathBuf::from(&a.path);
        let name = a.name().to_string();
        a.missing = model::is_broken(&a.path, a.kind);
        if a.missing {
            let msg = format!("NOT FOUND // {name} // PICK NEW LOCATION");
            self.toast(now, msg, EMBER);
            self.last_launch.insert(id, now);
            self.relocate(id, now);
            return;
        }
        self.last_launch.insert(id, now);
        match platform::launch(&path) {
            Ok(()) => {
                self.flashes.push((id, now));
                self.toast(now, format!("LAUNCH // {name}"), ACID);
            }
            Err(e) => self.toast(now, format!("LAUNCH FAILED // {e}"), EMBER),
        }
    }

    /// Ask the user where a missing item went; also re-links siblings that
    /// lived in the same old folder if the same file names exist in the new one.
    /// Ask the user where a missing item went. The dialog runs on its own
    /// thread so the window keeps drawing; the answer arrives through `pump`.
    fn relocate(&mut self, id: Id, now: f64) {
        if self.dialog_open {
            self.toast(now, "A FILE DIALOG IS ALREADY OPEN", ASH);
            return;
        }
        let Some((ci, ai)) = self.layout.find_app(id) else { return };
        let a = &self.layout.clusters[ci].apps[ai];
        let old = PathBuf::from(&a.path);
        let kind = a.kind;
        let ext = old.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        self.dialog_open = true;
        let (tx, ctx) = (self.res_tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            platform::thread_init();
            let start = old.ancestors().skip(1).find(|d| d.is_dir()).map(Path::to_path_buf);
            let picked = if kind == Kind::Dir {
                platform::pick_folder()
            } else {
                platform::pick_file(start.as_deref(), &ext, kind == Kind::Lnk)
            };
            let _ = tx.send(Res::Picked(id, picked));
            ctx.request_repaint();
        });
    }

    /// Point an item at its new location; also re-links siblings that lived in
    /// the same old folder if the same file names exist in the new one.
    fn apply_relocate(&mut self, id: Id, new: PathBuf, now: f64) {
        let Some((ci, ai)) = self.layout.find_app(id) else { return };
        self.snapshot();
        let old = PathBuf::from(&self.layout.clusters[ci].apps[ai].path);
        let old_dir = old.parent().map(Path::to_path_buf);
        let new_dir = new.parent().map(Path::to_path_buf);
        let new_s = new.to_string_lossy().into_owned();
        {
            let a = &mut self.layout.clusters[ci].apps[ai];
            self.icons.remove(&a.path);
            a.kind = Kind::of(&new);
            a.auto_name = meta::display_name(&new, a.kind);
            a.path = new_s;
            a.missing = model::is_broken(&a.path, a.kind);
        }
        let name = self.layout.clusters[ci].apps[ai].name().to_string();
        // Same folder moved? Fix the other missing items from it too.
        let mut extra = 0;
        if let (Some(od), Some(nd)) = (old_dir, new_dir) {
            if od != nd {
                for c in &mut self.layout.clusters {
                    for b in &mut c.apps {
                        let bp = PathBuf::from(&b.path);
                        if b.missing && bp.parent() == Some(od.as_path()) {
                            if let Some(f) = bp.file_name() {
                                let cand = nd.join(f);
                                if cand.exists() {
                                    self.icons.remove(&b.path);
                                    b.path = cand.to_string_lossy().into_owned();
                                    b.missing = model::is_broken(&b.path, b.kind);
                                    extra += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        let msg = if extra > 0 {
            format!("RELINKED // {name} + {extra} MORE FROM SAME FOLDER")
        } else {
            format!("RELINKED // {name}")
        };
        self.toast(now, msg, ACID);
        self.touch(now);
    }

    fn open_picker(&mut self, payload: Payload, world: [f32; 2], screen: Pos2, preselect: Option<Id>) {
        self.rename = None;
        let mut pk = Picker { payload, world, screen, query: String::new(), sel: 0, preselect, focus_frames: 3 };
        let opts = self.picker_options(&pk.query);
        pk.sel = preselect
            .and_then(|id| opts.iter().position(|o| matches!(o, PickChoice::Existing(c) if *c == id)))
            .unwrap_or(0);
        self.picker = Some(pk);
    }

    /// Existing clusters matching the query, plus a "new cluster" entry.
    fn picker_options(&self, query: &str) -> Vec<PickChoice> {
        let q = query.trim().to_lowercase();
        let mut out: Vec<PickChoice> = Vec::new();
        let mut exact = false;
        for c in &self.layout.clusters {
            let n = c.name.to_lowercase();
            if q.is_empty() || n.contains(&q) {
                exact |= n == q;
                out.push(PickChoice::Existing(c.id));
            }
        }
        if !exact {
            // Matches first so typing to search + Enter picks an existing cluster.
            out.push(PickChoice::New(query.trim().to_string()));
        }
        out
    }

    fn resolve_picker(&mut self, choice: PickChoice, now: f64) {
        let Some(pk) = self.picker.take() else { return };
        // One undo step for the whole action (new cluster + dock/move).
        self.snapshot();
        let depth = self.undo.len();
        let cid = match choice {
            PickChoice::Existing(id) => id,
            PickChoice::New(name) => {
                let name = if name.is_empty() { None } else { Some(name.to_uppercase()) };
                self.layout.add_cluster(name, pk.world)
            }
        };
        match pk.payload {
            Payload::Files(paths) => self.add_paths_to(cid, paths, now),
            Payload::Move(id) => self.move_app(id, cid, now),
        }
        self.undo.truncate(depth);
        self.touch(now);
    }

    fn paint_picker(&mut self, ctx: &egui::Context, canvas: Rect, now: f64) {
        let Some(pk) = &self.picker else { return };
        let what = match &pk.payload {
            Payload::Files(p) if p.len() == 1 => format!("DOCK // {}", meta::stem(&p[0])),
            Payload::Files(p) => format!("DOCK // {} FILES", p.len()),
            Payload::Move(id) => match self.layout.find_app(*id) {
                Some((ci, ai)) => format!("MOVE // {}", self.layout.clusters[ci].apps[ai].name()),
                None => "MOVE".into(),
            },
        };
        let w = 300.0;
        let pos = Pos2::new(
            pk.screen.x.clamp(canvas.min.x + 8.0, canvas.max.x - w - 8.0),
            pk.screen.y.clamp(canvas.min.y + 8.0, (canvas.max.y - 320.0).max(canvas.min.y + 8.0)),
        );
        let opts = self.picker_options(&pk.query);
        let mut sel = pk.sel.min(opts.len().saturating_sub(1));
        let (up, down, enter, esc) = ctx.input(|i| {
            (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape))
        });
        if up {
            sel = sel.saturating_sub(1);
        }
        if down && sel + 1 < opts.len() {
            sel += 1;
        }
        let mut chosen: Option<usize> = None;
        let mut cancel = esc;
        let clusters: HashMap<Id, (String, Color32, usize)> = self
            .layout
            .clusters
            .iter()
            .map(|c| (c.id, (c.name.clone(), accent(c.accent), c.apps.len())))
            .collect();
        let preselect = pk.preselect;
        let area = egui::Area::new(egui::Id::unique("picker"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(Stroke::new(2.0, ACID))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(w);
                        ui.horizontal(|ui| {
                            ui.label(spaced(&what.chars().take(30).collect::<String>(), mono(9.5), ACID, 1.2));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Label::new(spaced("ESC ×", mono(9.5), ASH, 1.0)).sense(Sense::click())).clicked() {
                                    cancel = true;
                                }
                            });
                        });
                        let pk = self.picker.as_mut().unwrap();
                        let te = ui.add(
                            egui::TextEdit::singleline(&mut pk.query)
                                .hint_text(spaced("SEARCH OR NAME A NEW CLUSTER", mono(11.0), ASH, 1.0))
                                .font(mono(13.0))
                                .desired_width(w)
                                .margin(egui::Margin::symmetric(6, 5)),
                        );
                        if pk.focus_frames > 0 {
                            te.request_focus();
                            pk.focus_frames -= 1;
                            ui.ctx().request_repaint();
                        }
                        if te.changed() {
                            sel = 0;
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                            for (i, o) in opts.iter().enumerate() {
                                let (label, col, sub) = match o {
                                    PickChoice::Existing(id) => {
                                        let (n, c, k) = clusters.get(id).cloned().unwrap_or_default();
                                        let near = if preselect == Some(*id) { "  · NEAREST" } else { "" };
                                        (n, c, format!("{k:02} NODES{near}"))
                                    }
                                    PickChoice::New(n) if n.is_empty() => ("NEW CLUSTER".into(), BONE, "AUTO-NAMED".into()),
                                    PickChoice::New(n) => (format!("+ {}", n.to_uppercase()), BONE, "CREATE NEW".into()),
                                };
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
                                let hot = i == sel || resp.hovered();
                                let p = ui.painter();
                                if hot {
                                    p.rect_filled(r, 0.0, if i == sel { ACID } else { RAISED });
                                }
                                let txt = if i == sel { VOID } else { BONE };
                                let chip = Rect::from_center_size(Pos2::new(r.min.x + 12.0, r.center().y), Vec2::splat(10.0));
                                if matches!(o, PickChoice::New(_)) {
                                    chamfer_stroke(p, chip, 3.0, Stroke::new(1.5, if i == sel { VOID } else { ACID }));
                                } else {
                                    chamfer_fill(p, chip, 3.0, col);
                                    if i == sel {
                                        chamfer_stroke(p, chip, 3.0, Stroke::new(1.5, VOID));
                                    }
                                }
                                let g = p.layout_job(spaced(&label, mono(11.5), txt, 1.0));
                                p.galley(Pos2::new(r.min.x + 26.0, r.center().y - g.size().y / 2.0), g, txt);
                                p.text(
                                    Pos2::new(r.max.x - 6.0, r.center().y),
                                    Align2::RIGHT_CENTER,
                                    sub,
                                    mono(9.0),
                                    if i == sel { VOID } else { ASH },
                                );
                                if resp.clicked() {
                                    chosen = Some(i);
                                }
                                if i == sel && (up || down) {
                                    resp.scroll_to_me(None);
                                }
                            }
                        });
                        ui.add_space(2.0);
                        ui.label(spaced("↑↓ SELECT  ·  ENTER DOCK", mono(9.0), ASH, 1.0));
                    });
            });
        if let Some(pk) = self.picker.as_mut() {
            pk.sel = sel;
        }
        // Click elsewhere cancels.
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| !area.response.rect.contains(p));
        if enter && !opts.is_empty() {
            chosen = Some(sel);
        }
        if let Some(i) = chosen {
            let choice = match &opts[i] {
                PickChoice::Existing(id) => PickChoice::Existing(*id),
                PickChoice::New(n) => PickChoice::New(n.clone()),
            };
            self.resolve_picker(choice, now);
        } else if cancel || clicked_outside {
            if let Some(Picker { payload: Payload::Move(_), .. }) = &self.picker {
                self.settled = false; // let the bubble spring home
            }
            self.picker = None;
        }
    }

    fn move_app(&mut self, id: Id, to: Id, now: f64) {
        let Some((ci, ai)) = self.layout.find_app(id) else { return };
        if self.layout.clusters[ci].id == to {
            return;
        }
        self.snapshot();
        let app = self.layout.clusters[ci].apps.remove(ai);
        let name = app.name().to_string();
        if let Some(c) = self.layout.cluster_mut(to) {
            let cname = c.name.clone();
            c.apps.push(app);
            self.toast(now, format!("{name} → {cname}"), BONE);
        }
        self.touch(now);
    }

    fn commit_rename(&mut self, now: f64) {
        let Some(r) = self.rename.take() else { return };
        self.snapshot();
        let text = r.text.trim().to_string();
        match r.target {
            Hit::Cluster(id) => {
                if let Some(c) = self.layout.cluster_mut(id) {
                    if !text.is_empty() {
                        c.name = text;
                    }
                }
            }
            Hit::App(id) => {
                if let Some((ci, ai)) = self.layout.find_app(id) {
                    let a = &mut self.layout.clusters[ci].apps[ai];
                    a.custom_name = if text.is_empty() || text == a.auto_name { None } else { Some(text) };
                }
            }
        }
        self.touch(now);
    }

    /// Icon texture if it has arrived; the first request queues a background load.
    fn icon(&mut self, path: &str) -> Option<egui::TextureId> {
        if let Some(t) = self.icons.get(path) {
            return t.as_ref().map(|t| t.id());
        }
        self.icons.insert(path.to_string(), None);
        let _ = self.req_tx.send(Req::Icon(path.to_string()));
        None
    }

    fn search_matches(&self, name: &str, path: &str) -> bool {
        let q = self.search.trim().to_lowercase();
        q.is_empty() || name.to_lowercase().contains(&q) || path.to_lowercase().contains(&q)
    }

    fn first_match(&self) -> Option<Id> {
        if self.search.trim().is_empty() {
            return None;
        }
        self.layout
            .clusters
            .iter()
            .flat_map(|c| c.apps.iter())
            .find(|a| self.search_matches(a.name(), &a.path))
            .map(|a| a.id)
    }

    // ------------------------------------------------------------- chrome

    fn toolbar(&mut self, ui: &mut egui::Ui, canvas_guess: Rect, now: f64) {
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            ui.label(spaced("HANGAR", display(17.0), BONE, 2.0));
            let (r, _) = ui.allocate_exact_size(Vec2::new(22.0, 16.0), Sense::hover());
            chamfer_fill(ui.painter(), r, 4.0, ACID);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "//", mono(11.0), VOID);
            ui.label(spaced(concat!("LAUNCH INDEX  v", env!("CARGO_PKG_VERSION")), mono(10.0), ASH, 1.5));
            ui.add_space(16.0);

            let search_id = egui::Id::unique("search");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id(search_id)
                    .hint_text(spaced("SEARCH  ·  CTRL+F", mono(11.0), ASH, 1.0))
                    .font(mono(12.0))
                    .desired_width(260.0)
                    .margin(egui::Margin::symmetric(8, 5)),
            );
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                if let Some(id) = self.first_match() {
                    self.launch(id, now);
                }
            }
            if resp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                self.search.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if hbutton(ui, "?", false).clicked() {
                    self.show_help = !self.show_help;
                }
                if hbutton(ui, "FIT", false).clicked() {
                    self.fit_view(canvas_guess);
                    self.touch(now);
                }
                if hbutton(ui, if self.layout.drift { "DRIFT ON" } else { "DRIFT OFF" }, self.layout.drift)
                    .on_hover_text("Ambient float. Pauses when the window is unfocused or minimised.")
                    .clicked()
                {
                    self.layout.drift = !self.layout.drift;
                    self.dirty_at.get_or_insert(now);
                }
                if false {
                    self.fit_view(canvas_guess);
                    self.touch(now);
                }
                if hbutton(ui, "+ CLUSTER", true).clicked() {
                    let xf = self.xf(canvas_guess);
                    let mut w = xf.w(canvas_guess.center());
                    w[0] += (self.layout.clusters.len() as f32 * 37.0) % 120.0 - 60.0;
                    let id = self.layout.add_cluster(None, w);
                    let name = self.layout.cluster_mut(id).map(|c| c.name.clone()).unwrap_or_default();
                    self.rename = Some(Rename { target: Hit::Cluster(id), text: name, focus_frames: 3 });
                    self.touch(now);
                }
            });
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            let txt = format!(
                "CL {:02}  //  NODES {:02}  //  ZOOM {:.2}x  //  {}",
                self.layout.clusters.len(),
                self.layout.app_count(),
                self.layout.view.zoom,
                if self.save_blocked.is_some() {
                    "READ-ONLY: LAYOUT FILE LOCKED"
                } else if self.save_failed {
                    "SAVE FAILED, RETRYING"
                } else if self.dirty_at.is_some() {
                    "SYNC…"
                } else {
                    "SAVED"
                }
            );
            ui.label(spaced(&txt, mono(10.0), ASH, 1.2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.label(spaced("DROP .EXE .LNK .HTML .BAT TO DOCK", mono(10.0), ASH, 1.2));
            });
        });
    }

    // ------------------------------------------------------------- canvas

    fn canvas(&mut self, ui: &mut egui::Ui, now: f64) {
        let ctx = ui.ctx().clone();
        let canvas = ui.max_rect();
        let resp = ui.allocate_rect(canvas, Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        if self.first_frame {
            self.first_frame = false;
            if self.layout.view.zoom <= 0.0 {
                self.layout.view = model::View::default();
            }
        }
        self.rebuild_plates(&painter);
        if let Some(paths) = self.pending.take() {
            self.dock_to_inbox(paths, now);
            self.undo.clear();
            self.rebuild_plates(&painter);
        }
        if let Some((text, color)) = self.startup_note.take() {
            self.toasts.push(Toast { text, color, until: now + 12.0 });
        }
        let mut xf = self.xf(canvas);

        // ---- zoom (wheel / pinch), anchored at the cursor
        if let Some(hp) = resp.hover_pos() {
            let (scroll, zd) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.0018).exp() * zd;
            if (factor - 1.0).abs() > 1e-4 {
                let before = xf.w(hp);
                let z = (self.layout.view.zoom * factor).clamp(0.15, 3.0);
                self.layout.view.zoom = z;
                let c = canvas.center();
                self.layout.view.pan = [hp.x - c.x - before[0] * z, hp.y - c.y - before[1] * z];
                xf = self.xf(canvas);
                self.dirty_at.get_or_insert(now);
            }
        }

        // ---- drag
        if resp.drag_started() {
            let origin = ctx.input(|i| i.pointer.press_origin()).unwrap_or(canvas.center());
            let middle = resp.dragged_by(PointerButton::Middle);
            self.drag = if middle {
                Some(Drag::Pan)
            } else {
                match self.hit(xf.w(origin), xf.zoom, None) {
                    Some(Hit::Cluster(id)) => Some(Drag::Cluster(id)),
                    Some(Hit::App(id)) => Some(Drag::App(id)),
                    None => Some(Drag::Pan),
                }
            };
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            let dw = [d.x / xf.zoom, d.y / xf.zoom];
            match self.drag {
                Some(Drag::Pan) => {
                    self.layout.view.pan[0] += d.x;
                    self.layout.view.pan[1] += d.y;
                    self.dirty_at.get_or_insert(now);
                }
                Some(Drag::Cluster(id)) => {
                    if let Some(c) = self.layout.cluster_mut(id) {
                        c.pos[0] += dw[0];
                        c.pos[1] += dw[1];
                        for a in &mut c.apps {
                            a.pos[0] += dw[0];
                            a.pos[1] += dw[1];
                        }
                    }
                    self.touch(now);
                }
                Some(Drag::App(id)) => {
                    if let Some((ci, ai)) = self.layout.find_app(id) {
                        let a = &mut self.layout.clusters[ci].apps[ai];
                        a.pos[0] += dw[0];
                        a.pos[1] += dw[1];
                    }
                    self.touch(now);
                }
                None => {}
            }
            xf = self.xf(canvas);
        }
        let mut drop_target: Option<Id> = None;
        if let (Some(Drag::App(id)), Some(p)) = (self.drag, resp.interact_pointer_pos().or(resp.hover_pos())) {
            let own = self.layout.find_app(id).map(|(ci, _)| self.layout.clusters[ci].id);
            drop_target = self.drop_cluster(xf.w(p), xf.zoom, Some(Hit::App(id))).filter(|c| Some(*c) != own);
        }
        if resp.drag_stopped() {
            if let (Some(Drag::App(id)), Some(to)) = (self.drag, drop_target) {
                self.move_app(id, to, now);
            } else if let Some(Drag::App(id)) = self.drag {
                // Released far out in open space: ask where it should live.
                if let Some((ci, ai)) = self.layout.find_app(id) {
                    let c = &self.layout.clusters[ci];
                    let a = &c.apps[ai];
                    let d = Vec2::new(a.pos[0] - c.pos[0], a.pos[1] - c.pos[1]).length();
                    if d > c.orbit() + 140.0 {
                        let (w, sp) = (a.pos, xf.s(a.pos));
                        self.open_picker(Payload::Move(id), w, sp, None);
                    }
                }
            }
            self.drag = None;
            drop_target = None;
        }
        let pinned = match self.drag {
            Some(Drag::App(id)) | Some(Drag::Cluster(id)) => Some(id),
            _ => None,
        };

        // ---- hover / click
        let hover = resp.hover_pos().and_then(|p| self.hit(xf.w(p), xf.zoom, None));
        if hover.is_some() && self.drag.is_none() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        } else if matches!(self.drag, Some(Drag::Pan)) {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        }
        if resp.clicked() {
            if let Some(Hit::App(id)) = hover {
                self.launch(id, now);
            }
        }
        if resp.double_clicked() {
            if let Some(Hit::Cluster(id)) = hover {
                let name = self.layout.cluster_mut(id).map(|c| c.name.clone()).unwrap_or_default();
                self.rename = Some(Rename { target: Hit::Cluster(id), text: name, focus_frames: 3 });
            }
        }
        if resp.secondary_clicked() {
            self.ctx_target = hover;
            self.ctx_world = resp.hover_pos().map(|p| xf.w(p)).unwrap_or([0.0; 2]);
        }
        resp.context_menu(|ui| self.context_menu(ui, canvas, now));

        // ---- keyboard
        let text_focus = ctx.memory(|m| m.focused().is_some());
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, Key::F) {
                self.focus_search = true;
            }
        });
        if self.focus_search {
            ctx.request_repaint();
        }
        ctx.input_mut(|_| {
        });
        if !text_focus && self.rename.is_none() && self.picker.is_none() {
            if ctx.input(|i| i.key_pressed(Key::F)) {
                self.fit_view(canvas);
                xf = self.xf(canvas);
                self.dirty_at.get_or_insert(now);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Z)) {
                self.undo(now);
                self.rebuild_plates(&painter);
                xf = self.xf(canvas);
            }
            if ctx.input(|i| i.key_pressed(Key::F2)) {
                if let Some(h) = hover {
                    self.start_rename(h);
                }
            }
        }

        // ---- physics (only runs until settled -> zero idle CPU)
        if !self.settled {
            let mut v = 0.0;
            for _ in 0..2 {
                v = model::step(&mut self.layout, pinned);
            }
            if v < 0.03 && self.drag.is_none() {
                self.settled = true;
            } else {
                ctx.request_repaint();
            }
        }

        // ---- files hovering / dropped
        let (hovering, dropped): (usize, Vec<PathBuf>) = ctx.input(|i| {
            (
                i.raw.hovered_files.len(),
                i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect(),
            )
        });
        let os_pointer = || -> Option<Pos2> {
            let (x, y) = platform::cursor_screen_pos()?;
            let ppp = ctx.pixels_per_point();
            let inner = ctx.input(|i| i.viewport().inner_rect)?;
            Some(Pos2::new(x / ppp, y / ppp) - inner.min.to_vec2())
        };
        let file_pos = os_pointer()
            .filter(|p| canvas.contains(*p))
            .or_else(|| ctx.input(|i| i.pointer.latest_pos()))
            .unwrap_or(canvas.center());
        // Dropping straight onto a plate docks there; anywhere else opens the picker.
        let plate_at = |me: &Self, p: Pos2| match me.hit(xf.w(p), xf.zoom, None) {
            Some(Hit::Cluster(id)) => Some(id),
            _ => None,
        };
        let mut file_target = None;
        if hovering > 0 {
            file_target = plate_at(self, file_pos);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if !dropped.is_empty() {
            match plate_at(self, file_pos) {
                Some(cid) => self.add_paths_to(cid, dropped, now),
                None => {
                    let pre = self.drop_cluster(xf.w(file_pos), xf.zoom, None);
                    self.open_picker(Payload::Files(dropped), xf.w(file_pos), file_pos, pre);
                }
            }
            self.rebuild_plates(&painter);
        }

        // ================================================================ paint
        self.paint_background(&painter, canvas, xf);
        let highlight = drop_target.or(file_target);
        self.paint_graph(&ctx, &painter, xf, hover, highlight, now);
        self.paint_rename(&ctx, xf, now);
        self.paint_picker(&ctx, canvas, now);

        if self.layout.clusters.is_empty() {
            paint_empty(&painter, canvas);
        }
        if hovering > 0 {
            let tname = file_target.and_then(|id| self.layout.clusters.iter().find(|c| c.id == id)).map(|c| c.name.clone());
            paint_drop_overlay(&painter, canvas, hovering, tname.as_deref());
        }
        brackets(&painter, canvas.shrink(10.0), 18.0, Stroke::new(1.5, LINE));
    }

    fn start_rename(&mut self, h: Hit) {
        let text = match h {
            Hit::Cluster(id) => self.layout.cluster_mut(id).map(|c| c.name.clone()),
            Hit::App(id) => self
                .layout
                .find_app(id)
                .map(|(ci, ai)| self.layout.clusters[ci].apps[ai].name().to_string()),
        };
        if let Some(text) = text {
            self.rename = Some(Rename { target: h, text, focus_frames: 3 });
        }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, canvas: Rect, now: f64) {
        ui.set_min_width(200.0);
        match self.ctx_target {
            Some(Hit::App(id)) => {
                let Some((ci, ai)) = self.layout.find_app(id) else { return };
                let a = &self.layout.clusters[ci].apps[ai];
                let (path, has_custom, own) = (a.path.clone(), a.custom_name.is_some(), self.layout.clusters[ci].id);
                let kind = a.kind;
                menu_header(ui, &format!("{} // {}", a.kind.tag(), a.name()));
                if ui.button("LAUNCH").clicked() {
                    self.launch(id, now);
                    ui.close();
                }
                if ui.button("RELOCATE…").clicked() {
                    ui.close();
                    self.relocate(id, now);
                    return;
                }
                if ui.button("OPEN FILE LOCATION").clicked() {
                    let target = if kind == Kind::Lnk {
                        meta::lnk_target(Path::new(&path)).unwrap_or(path.clone())
                    } else {
                        path.clone()
                    };
                    if let Err(e) = platform::reveal(Path::new(&target)) {
                        self.toast(now, e, EMBER);
                    }
                    ui.close();
                }
                if ui.button("RENAME   F2").clicked() {
                    self.start_rename(Hit::App(id));
                    ui.close();
                }
                if has_custom && ui.button("RESET NAME").clicked() {
                    let (ci, ai) = self.layout.find_app(id).unwrap();
                    let a = &mut self.layout.clusters[ci].apps[ai];
                    let p = PathBuf::from(&a.path);
                    a.auto_name = meta::display_name(&p, a.kind);
                    a.custom_name = None;
                    self.touch(now);
                    ui.close();
                }
                let others: Vec<(Id, String)> = self
                    .layout
                    .clusters
                    .iter()
                    .filter(|c| c.id != own)
                    .map(|c| (c.id, c.name.clone()))
                    .collect();
                if !others.is_empty() {
                    ui.menu_button("MOVE TO", |ui| {
                        for (cid, name) in others {
                            if ui.button(name.to_uppercase()).clicked() {
                                self.move_app(id, cid, now);
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button(egui::RichText::new("REMOVE FROM DOCK").color(EMBER)).clicked() {
                    let (ci, ai) = self.layout.find_app(id).unwrap();
                    self.snapshot();
                    self.layout.clusters[ci].apps.remove(ai);
                    self.toast(now, "REMOVED // CTRL+Z TO UNDO", BONE);
                    self.touch(now);
                    ui.close();
                }
            }
            Some(Hit::Cluster(id)) => {
                let Some(c) = self.layout.cluster_mut(id) else { return };
                let (name, n) = (c.name.clone(), c.apps.len());
                menu_header(ui, &format!("CLUSTER // {name}"));
                if ui.button("RENAME").clicked() {
                    self.start_rename(Hit::Cluster(id));
                    ui.close();
                }
                ui.menu_button("ACCENT", |ui| {
                    for (i, nm) in ACCENT_NAMES.iter().enumerate() {
                        let rt = egui::RichText::new(format!("■  {nm}")).color(ACCENTS[i]);
                        if ui.button(rt).clicked() {
                            if let Some(c) = self.layout.cluster_mut(id) {
                                c.accent = i as u8;
                            }
                            self.touch(now);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                // No confirm step: the menu closes on click, and Ctrl+Z restores the cluster.
                let label = if n == 0 { "DELETE CLUSTER".to_string() } else { format!("DELETE CLUSTER + {n} NODES") };
                if ui.button(egui::RichText::new(label).color(EMBER)).clicked() {
                    self.snapshot();
                    self.layout.clusters.retain(|c| c.id != id);
                    self.toast(now, format!("DELETED // {name} // CTRL+Z TO UNDO"), EMBER);
                    self.touch(now);
                    ui.close();
                }
                ui.label(spaced("FILES ON DISK ARE NEVER TOUCHED", mono(9.0), ASH, 1.0));
            }
            None => {
                menu_header(ui, "HANGAR");
                if ui.button("NEW CLUSTER HERE").clicked() {
                    let id = self.layout.add_cluster(None, self.ctx_world);
                    self.start_rename(Hit::Cluster(id));
                    self.touch(now);
                    ui.close();
                }
                if ui.button("FIT VIEW   F").clicked() {
                    self.fit_view(canvas);
                    self.touch(now);
                    ui.close();
                }
                if ui.button("OPEN DATA FOLDER").clicked() {
                    let _ = platform::reveal(&self.data_path.clone());
                    ui.close();
                }
            }
        }
    }

    fn paint_background(&self, p: &Painter, canvas: Rect, xf: Xf) {
        // Dot grid: pick a power-of-two multiple of 40 so dots stay >= 22px apart.
        let mut step = 40.0;
        while step * xf.zoom < 22.0 {
            step *= 2.0;
        }
        let tl = xf.w(canvas.min);
        let br = xf.w(canvas.max);
        let x0 = (tl[0] / step).floor() as i64;
        let x1 = (br[0] / step).ceil() as i64;
        let y0 = (tl[1] / step).floor() as i64;
        let y1 = (br[1] / step).ceil() as i64;
        let major = (200.0 / step).round().max(1.0) as i64;
        for gx in x0..=x1 {
            for gy in y0..=y1 {
                let s = xf.s([gx as f32 * step, gy as f32 * step]);
                if major > 0 && gx % (major * 5).max(5) == 0 && gy % (major * 5).max(5) == 0 && step <= 200.0 {
                    reg_mark(p, s, 5.0, Stroke::new(1.0, LINE));
                } else {
                    p.rect_filled(Rect::from_center_size(s, Vec2::splat(2.0)), 0.0, GRID);
                }
            }
        }
        // Origin crosshair
        let o = xf.s([0.0, 0.0]);
        if canvas.expand(20.0).contains(o) {
            reg_mark(p, o, 10.0, Stroke::new(1.0, ASH));
            p.text(o + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, "00.00", mono(9.0), LINE);
        }
    }

    fn paint_graph(
        &mut self,
        ctx: &egui::Context,
        p: &Painter,
        xf: Xf,
        hover: Option<Hit>,
        drop_target: Option<Id>,
        now: f64,
    ) {
        let z = xf.zoom;
        let searching = !self.search.trim().is_empty();
        self.flashes.retain(|(_, t)| now - t < 0.35);
        if !self.flashes.is_empty() {
            ctx.request_repaint();
        }
        let plates = self.plates.clone();

        // Orbits + edges first.
        for c in &self.layout.clusters {
            let col = accent(c.accent);
            let center = xf.s(c.pos);
            let r = c.orbit().max(70.0) * z;
            let is_target = drop_target == Some(c.id);
            let dragging = self.drag == Some(Drag::Cluster(c.id));
            if is_target {
                p.circle_filled(center, r + BUBBLE_R * z + 14.0 * z, with_alpha(ACID, 0.06));
                p.circle_stroke(center, r + BUBBLE_R * z + 14.0 * z, Stroke::new(2.0, ACID));
            } else if dragging {
                p.circle_stroke(center, r, Stroke::new(1.0, with_alpha(col, 0.5)));
            } else if !c.apps.is_empty() {
                let pts: Vec<Pos2> = (0..=96)
                    .map(|i| {
                        let a = i as f32 / 96.0 * std::f32::consts::TAU;
                        center + Vec2::angled(a) * r
                    })
                    .collect();
                p.extend(Shape::dashed_line(&pts, Stroke::new(1.0, with_alpha(col, 0.22)), 6.0, 6.0));
            }
            for a in &c.apps {
                let ap = xf.s(a.pos);
                let delta = ap - center;
                if delta.length_sq() < 1.0 {
                    continue; // on top of the plate: nothing to draw, and avoids a NaN direction
                }
                let dir = delta.normalized();
                let end = ap - dir * BUBBLE_R * z;
                let dim = if searching && !self.search_matches(a.name(), &a.path) { 0.1 } else { 0.35 };
                p.line_segment([center, end], Stroke::new(1.0, with_alpha(col, dim)));
                p.rect_filled(Rect::from_center_size(end, Vec2::splat(4.0)), 0.0, with_alpha(col, dim * 2.0));
            }
        }

        // Bubbles.
        let t = now as f32;
        let mut bubble_jobs = Vec::new();
        for c in &self.layout.clusters {
            for a in &c.apps {
                bubble_jobs.push((accent(c.accent), a.id, a.pos, a.name().to_string(), a.path.clone(), a.kind, a.missing));
            }
        }
        let mut hover_anim = false;
        for (col, id, pos, name, path, kind, missing) in bubble_jobs {
            let s = xf.s(pos);
            let r = BUBBLE_R * z;
            if !p.clip_rect().expand(r + 120.0 * z).contains(s) {
                continue;
            }
            let matched = searching && self.search_matches(&name, &path);
            let dim = if searching && !matched { 0.25 } else { 1.0 };
            let hovered = hover == Some(Hit::App(id)) && self.drag.is_none();
            let dragged = self.drag == Some(Drag::App(id));
            p.circle_filled(s, r, with_alpha(PANEL, if dim < 1.0 { 0.6 } else { 1.0 }));
            if missing {
                let pts: Vec<Pos2> = (0..=48).map(|i| s + Vec2::angled(i as f32 / 48.0 * std::f32::consts::TAU) * r).collect();
                p.extend(Shape::dashed_line(&pts, Stroke::new(1.5, with_alpha(EMBER, dim)), 4.0, 3.0));
            } else if hovered || dragged || matched {
                p.circle_stroke(s, r, Stroke::new(2.0, col));
            } else {
                p.circle_stroke(s, r, Stroke::new(1.5, with_alpha(LINE, dim.max(0.5))));
            }
            if hovered {
                hover_anim = true;
                for k in 0..4 {
                    let a0 = t * 0.6 + k as f32 * std::f32::consts::FRAC_PI_2;
                    let pts: Vec<Pos2> = (0..=6).map(|i| s + Vec2::angled(a0 + i as f32 * 0.08) * (r + 6.0 * z.max(0.6))).collect();
                    p.add(Shape::line(pts, Stroke::new(2.0, col)));
                }
            }
            // Icon or monogram.
            let icon_sz = (r * 1.25).max(8.0);
            match self.icon(&path) {
                Some(tex) => {
                    let tint = Color32::WHITE.gamma_multiply(dim);
                    p.image(
                        tex,
                        Rect::from_center_size(s, Vec2::splat(icon_sz)),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        tint,
                    );
                }
                None => {
                    let mono_txt = monogram(&name);
                    p.text(s, Align2::CENTER_CENTER, mono_txt, display(17.0 * z), with_alpha(col, dim));
                }
            }
            // Flash
            for (fid, ft) in &self.flashes {
                if *fid == id {
                    let k = ((now - ft) / 0.35) as f32;
                    p.circle_stroke(s, r + 30.0 * z * k, Stroke::new(3.0 * (1.0 - k), col));
                }
            }
            // Labels
            if z >= 0.45 {
                let label: String = if name.chars().count() > 22 {
                    name.chars().take(21).collect::<String>() + "…"
                } else {
                    name.clone()
                };
                let g = p.layout_job(spaced(&label, mono(11.5 * z), with_alpha(BONE, dim), 1.0 * z));
                let lp = s + Vec2::new(-g.size().x / 2.0, r + 6.0 * z);
                if hovered {
                    p.rect_filled(Rect::from_min_size(lp, g.size()).expand(3.0 * z), 0.0, VOID);
                }
                p.galley(lp, g.clone(), BONE);
                let tag = if missing { format!("{} // NOT FOUND", kind.tag()) } else { kind.tag().to_string() };
                let tc = if missing { EMBER } else { ASH };
                p.text(
                    lp + Vec2::new(g.size().x / 2.0, g.size().y + 2.0 * z),
                    Align2::CENTER_TOP,
                    tag,
                    mono(9.0 * z),
                    with_alpha(tc, dim),
                );
            }
        }
        if hover_anim {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        // Plates on top.
        for c in &self.layout.clusters {
            let Some((_, wr)) = plates.iter().find(|(id, _)| *id == c.id) else { continue };
            let col = accent(c.accent);
            let r = Rect::from_min_max(xf.s([wr.min.x, wr.min.y]), xf.s([wr.max.x, wr.max.y]));
            let renaming = matches!(&self.rename, Some(rn) if rn.target == Hit::Cluster(c.id));
            if hover == Some(Hit::Cluster(c.id)) || self.drag == Some(Drag::Cluster(c.id)) {
                chamfer_stroke(p, r.expand(4.0), 10.0 * z + 2.0, Stroke::new(2.0, BONE));
            }
            if self.drag == Some(Drag::Cluster(c.id)) {
                chamfer_fill(p, r.translate(Vec2::new(5.0, 5.0)), 8.0 * z, with_alpha(col, 0.25));
            }
            chamfer_fill(p, r, 8.0 * z, col);
            // hazard ticks at the right end
            for k in 0..3 {
                let x = r.max.x - (8.0 + k as f32 * 5.0) * z;
                p.line_segment(
                    [Pos2::new(x, r.min.y + 5.0 * z), Pos2::new(x - 4.0 * z, r.min.y + 11.0 * z)],
                    Stroke::new(1.5 * z.max(0.5), with_alpha(VOID, 0.55)),
                );
            }
            if !renaming {
                let g = p.layout_job(spaced(&c.name, display(20.0 * z), VOID, z));
                p.galley(r.center() - g.size() / 2.0 + Vec2::new(-9.0 * z, 0.0), g, VOID);
            }
            if z >= 0.4 {
                let idx = self.layout.clusters.iter().position(|x| x.id == c.id).unwrap_or(0);
                let code = format!("CL-{:02} // {:02} NODES", idx + 1, c.apps.len());
                let g = p.layout_job(spaced(&code, mono(9.5 * z), ASH, 1.2 * z));
                p.galley(Pos2::new(r.min.x, r.max.y + 4.0 * z), g, ASH);
            }
        }
    }

    fn paint_rename(&mut self, ctx: &egui::Context, xf: Xf, now: f64) {
        let Some(rn) = &self.rename else { return };
        let (pos, width) = match rn.target {
            Hit::Cluster(id) => match self.plates.iter().find(|(i, _)| *i == id) {
                Some((_, r)) => (xf.s([r.min.x, r.min.y]), (r.width() * xf.zoom).max(220.0)),
                None => {
                    self.rename = None;
                    return;
                }
            },
            Hit::App(id) => match self.layout.find_app(id) {
                Some((ci, ai)) => {
                    let a = &self.layout.clusters[ci].apps[ai];
                    (xf.s(a.pos) + Vec2::new(-120.0, BUBBLE_R * xf.zoom + 4.0), 240.0)
                }
                None => {
                    self.rename = None;
                    return;
                }
            },
        };
        let mut commit = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::unique("rename"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(2.0, ACID))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.label(spaced("RENAME  ·  ENTER / ESC", mono(9.0), ACID, 1.2));
                        let rn = self.rename.as_mut().unwrap();
                        let te = ui.add(
                            egui::TextEdit::singleline(&mut rn.text)
                                .font(mono(14.0))
                                .desired_width(width)
                                .frame(egui::Frame::NONE),
                        );
                        if rn.focus_frames > 0 {
                            // Areas are invisible on their first frame; keep asking for focus briefly.
                            te.request_focus();
                            rn.focus_frames -= 1;
                            ui.ctx().request_repaint();
                        }
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            cancel = true;
                        } else if te.lost_focus() {
                            commit = true;
                        }
                    });
            });
        if cancel {
            self.rename = None;
        } else if commit {
            self.commit_rename(now);
        }
    }

    fn paint_toasts(&mut self, ctx: &egui::Context, screen: Rect, now: f64) {
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("toasts")));
        let mut y = screen.max.y - 40.0;
        for t in self.toasts.iter().rev() {
            let a = ((t.until - now) / 0.3).clamp(0.0, 1.0) as f32;
            let g = p.layout_job(spaced(&t.text, mono(10.5), with_alpha(BONE, a), 1.2));
            let w = g.size().x + 28.0;
            let r = Rect::from_min_size(Pos2::new(screen.max.x - w - 18.0, y - 30.0), Vec2::new(w, 30.0));
            chamfer_fill(&p, r, 6.0, with_alpha(PANEL, a));
            chamfer_stroke(&p, r, 6.0, Stroke::new(1.0, with_alpha(LINE, a)));
            p.rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 6.0), Vec2::new(4.0, 24.0)), 0.0, with_alpha(t.color, a));
            p.galley(Pos2::new(r.min.x + 16.0, r.center().y - g.size().y / 2.0), g, BONE);
            y -= 38.0;
        }
        let next = self.toasts.iter().map(|t| t.until).fold(f64::MAX, f64::min);
        // Sleep until the next toast starts fading; animate only during the 0.3 s fade.
        let wait = next - now - 0.3;
        if wait > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.min(3600.0)));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new(spaced("CONTROLS", display(15.0), BONE, 1.5))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, Vec2::new(-16.0, 52.0))
            .show(ctx, |ui| {
                for (k, v) in [
                    ("DROP FILES", "On a cluster: dock there. On empty space: new cluster"),
                    ("CLICK NODE", "Launch"),
                    ("DRAG NODE", "Move; release over another cluster to re-dock"),
                    ("DRAG PLATE", "Move a whole cluster"),
                    ("DBL-CLICK PLATE", "Rename cluster"),
                    ("RIGHT-CLICK", "Node / cluster / canvas menu"),
                    ("DRAG EMPTY", "Pan (middle mouse anywhere)"),
                    ("WHEEL", "Zoom at cursor"),
                    ("CTRL+F", "Search; ENTER launches first match"),
                    ("F / F2", "Fit view / rename hovered"),
                    ("CTRL+Z", "Undo dock, move, rename, remove, delete (30 steps)"),
                ] {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(130.0, 16.0), Sense::hover());
                        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, k, mono(10.5), ACID);
                        ui.label(egui::RichText::new(v).font(mono(10.5)).color(BONE));
                    });
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(format!("DATA  {}", self.data_path.display())).font(mono(9.5)).color(ASH));
            });
        self.show_help = open;
    }
}

/// Two-letter monogram: word initials, or camelCase humps ("UnrealEditor" -> "UE").
fn monogram(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut out: String = words.iter().filter_map(|w| w.chars().next()).take(2).collect();
    if out.chars().count() < 2 {
        if let Some(w) = words.first() {
            let humps: String = w.chars().filter(|c| c.is_uppercase()).take(2).collect();
            if humps.chars().count() == 2 {
                out = humps;
            } else {
                out = w.chars().take(2).collect();
            }
        }
    }
    out.to_uppercase()
}

fn menu_header(ui: &mut egui::Ui, text: &str) {
    let t: String = if text.chars().count() > 34 { text.chars().take(33).collect::<String>() + "…" } else { text.to_string() };
    ui.label(spaced(&t, mono(9.5), ASH, 1.2));
    ui.separator();
}

/// Chamfered toolbar button.
fn hbutton(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let g = ui.painter().layout_job(spaced(label, mono(11.0), BONE, 1.2));
    let size = Vec2::new(g.size().x + 22.0, 26.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let hot = resp.hovered();
    if hot || primary {
        chamfer_fill(p, r, 6.0, if hot { ACID } else { RAISED });
    }
    chamfer_stroke(p, r, 6.0, Stroke::new(1.0, if hot { ACID } else if primary { ACID } else { LINE }));
    let col = if hot { VOID } else { BONE };
    let g = p.layout_job(spaced(label, mono(11.0), col, 1.2));
    p.galley(r.center() - g.size() / 2.0, g, col);
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

fn paint_empty(p: &Painter, canvas: Rect) {
    let c = canvas.center();
    let r = Rect::from_center_size(c, Vec2::new(520.0, 150.0));
    chamfer_stroke(p, r, 14.0, Stroke::new(1.0, LINE));
    let tag = Rect::from_min_size(r.min + Vec2::new(18.0, -11.0), Vec2::new(120.0, 22.0));
    chamfer_fill(p, tag, 5.0, ACID);
    p.text(tag.center(), Align2::CENTER_CENTER, "STATUS // IDLE", mono(10.0), VOID);
    let g = p.layout_job(spaced("DOCK EMPTY", display(30.0), BONE, 3.0));
    p.galley(Pos2::new(c.x - g.size().x / 2.0, c.y - 38.0), g, BONE);
    let g = p.layout_job(spaced("DRAG .EXE  .LNK  .HTML  .BAT ONTO THE GRID", mono(11.0), ASH, 1.5));
    p.galley(Pos2::new(c.x - g.size().x / 2.0, c.y + 10.0), g, ASH);
    let g = p.layout_job(spaced("DROP ANYWHERE, THEN PICK OR NAME A CLUSTER", mono(10.0), ASH, 1.5));
    p.galley(Pos2::new(c.x - g.size().x / 2.0, c.y + 32.0), g, ASH);
}

fn paint_drop_overlay(p: &Painter, canvas: Rect, n: usize, target: Option<&str>) {
    let r = canvas.shrink(6.0);
    p.rect_stroke(r, 0.0, Stroke::new(2.0, ACID), egui::StrokeKind::Inside);
    // hazard stripes in the corners
    for corner in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
        let sx = if corner.x < canvas.center().x { 1.0 } else { -1.0 };
        let sy = if corner.y < canvas.center().y { 1.0 } else { -1.0 };
        for k in 0..4 {
            let o = 10.0 + k as f32 * 10.0;
            p.line_segment(
                [corner + Vec2::new(sx * o, sy * 2.0), corner + Vec2::new(sx * 2.0, sy * o)],
                Stroke::new(3.0, ACID),
            );
        }
    }
    let plural = if n == 1 { "" } else { "S" };
    let text = match target {
        Some(name) => format!("DOCK TO {name} // {n} FILE{plural}"),
        None => format!("DROP // PICK OR NAME A CLUSTER // {n} FILE{plural}"),
    };
    let g = p.layout_job(spaced(&text, display(16.0), VOID, 1.5));
    let pr = Rect::from_center_size(Pos2::new(canvas.center().x, canvas.min.y + 40.0), g.size() + Vec2::new(36.0, 16.0));
    chamfer_fill(p, pr, 8.0, ACID);
    p.galley(pr.center() - g.size() / 2.0, g, VOID);
}

impl eframe::App for HangarApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let screen = ui.max_rect();
        // Approximate canvas rect for toolbar actions (exact one is known later).
        let canvas_guess = Rect::from_min_max(screen.min + Vec2::new(0.0, 40.0), screen.max - Vec2::new(0.0, 24.0));

        // Ambient drift: offsets are applied for this frame only, then removed,
        // so hit-testing matches what is drawn and saved positions stay put.
        let (focused, minimized) =
            ctx.input(|i| (i.viewport().focused.unwrap_or(true), i.viewport().minimized.unwrap_or(false)));
        self.pump(now, focused);
        if let Some(n) = self.selftest_frames.as_mut() {
            *n += 1;
            if *n >= 5 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
        // Remember the window size for next start.
        let (inner, maxed) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized.unwrap_or(false)));
        if !minimized {
            if maxed != self.layout.maximized {
                self.layout.maximized = maxed;
                self.dirty_at.get_or_insert(now);
            }
            if let (false, Some(r)) = (maxed, inner) {
                let sz = [r.width().round(), r.height().round()];
                if sz[0] >= 320.0 && sz[1] >= 240.0 && self.layout.window != Some(sz) {
                    self.layout.window = Some(sz);
                    self.dirty_at.get_or_insert(now);
                }
            }
        }
        let drifting = self.layout.drift && focused && !minimized;
        self.drift_applied.clear();
        if drifting {
            model::apply_drift(&mut self.layout, now, &mut self.drift_applied);
        }

        egui::Panel::top("bar")
            .exact_size(40.0)
            .frame(egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| self.toolbar(ui, canvas_guess, now));
        egui::Panel::bottom("status")
            .exact_size(24.0)
            .frame(egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(VOID))
            .show(ui, |ui| self.canvas(ui, now));

        if drifting {
            model::unapply_drift(&mut self.layout, &self.drift_applied);
            self.drift_applied.clear();
            // 20 fps is plenty for a ~0.2 Hz drift and keeps CPU low.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        if self.show_help {
            self.help_window(&ctx);
        }
        self.paint_toasts(&ctx, screen, now);

        // Debounced autosave.
        if let Some(t) = self.dirty_at {
            if now - t > 1.0 && self.drag.is_none() {
                self.save_now(now);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(1100));
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.dirty_at.is_some() {
            self.save_now(0.0);
        }
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        VOID.to_normalized_gamma_f32()
    }
}

