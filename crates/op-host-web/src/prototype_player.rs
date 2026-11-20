use crate::canvaskit::{init_backend, BrowserMeasure, CanvasKitBackend};
use jian_ops_schema::node::PenNode;
use jian_ops_schema::PenDocument;
use op_editor_core::PenNodeExt;
use op_editor_ui::{Color, Point2D, Rect, RenderBackend};
use op_preview_core::{InvalidationKind, PreviewSession};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct PrototypePlayer {
    backend: CanvasKitBackend,
    session: Option<PreviewSession>,
    entry: Option<String>,
    flows: HashMap<String, String>,
    region: Rect,
    origin: Point2D,
    scale: f32,
    fit: Option<bool>,
    animating: bool,
}

#[wasm_bindgen]
pub async fn mount_prototype_player(
    canvas_id: String,
    width: u32,
    height: u32,
    dpr: f32,
) -> Result<PrototypePlayer, JsValue> {
    console_error_panic_hook::set_once();
    let mut backend = init_backend(&canvas_id, dpr, width, height).await?;
    backend.resize_for_display(width, height, dpr);
    Ok(PrototypePlayer {
        backend,
        session: None,
        entry: None,
        flows: HashMap::new(),
        region: Rect::ZERO,
        origin: Point2D::ZERO,
        scale: 1.0,
        fit: None,
        animating: false,
    })
}

#[wasm_bindgen]
impl PrototypePlayer {
    pub fn load(&mut self, source: &str, now: f64) -> Result<(), JsValue> {
        let meta = op_pen_loader::extract_editor_meta(source);
        let mut document = op_pen_loader::load_canonical(source)
            .map_err(js_error)?
            .value;
        let entry = document.routes.as_ref().map(|routes| routes.entry.clone());
        prepare_screens(&mut document);
        let flows = document
            .children
            .iter()
            .chain(
                document
                    .pages
                    .iter()
                    .flatten()
                    .flat_map(|page| &page.children),
            )
            .filter_map(|node| {
                let PenNode::Frame(frame) = node else {
                    return None;
                };
                let name = frame
                    .gestures
                    .as_ref()?
                    .extra
                    .get("prototypeFlow")?
                    .as_str()?;
                Some((frame.screen.clone()?, name.to_owned()))
            })
            .collect();
        let mut session = PreviewSession::enter(
            &document,
            self.backend.logical_size(),
            &BTreeMap::new(),
            0,
            meta.is_some_and(|meta| meta.preserve_authored_geometry),
            false,
            Rc::new(BrowserMeasure::new(self.backend.op_ck())),
            now as u64,
        )
        .map_err(js_error)?;
        if !session.is_app_mode() || session.framed_root().is_none() {
            return Err(JsValue::from_str("没有可播放的原型页面"));
        }
        session.enable_prototype_viewport();
        session.begin_lifecycle(now as u64);
        self.entry = entry;
        self.flows = flows;
        self.session = Some(session);
        self.restore_entry(now as u64, None);
        Ok(())
    }

    pub fn register_font(&mut self, bytes: &[u8]) {
        if let Some(family) = crate::font_meta::parse_family(bytes) {
            self.backend.register_bundled_font(&family, bytes);
        }
    }

    pub fn screens(&self) -> String {
        let screens = self
            .session
            .iter()
            .flat_map(|session| session.screen_switcher_entries())
            .map(|(path, name)| {
                let flow = self.flows.get(&path);
                serde_json::json!({ "path": path, "name": name, "flow": flow })
            })
            .collect::<Vec<_>>();
        serde_json::Value::Array(screens).to_string()
    }

    pub fn screen_index(&self) -> usize {
        self.session
            .as_ref()
            .and_then(PreviewSession::current_screen_index)
            .unwrap_or(0)
    }

    pub fn restart(&mut self, path: Option<String>, now: f64) -> Result<(), JsValue> {
        if let Some(session) = &mut self.session {
            session.reset().map_err(js_error)?;
            session.enable_prototype_viewport();
        }
        self.restore_entry(now as u64, path.as_deref());
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32, dpr: f32) {
        self.backend.resize_for_display(width, height, dpr);
        self.update_viewport();
    }

    pub fn fit_entire_frame(&mut self, fit: bool) {
        self.fit = Some(fit);
        self.update_viewport();
    }

    pub fn fits_entire_frame(&self) -> bool {
        self.fit.unwrap_or_else(|| {
            self.session
                .as_ref()
                .is_some_and(PreviewSession::prototype_has_authored_viewport)
        })
    }

    pub fn tap(&mut self, x: f32, y: f32, now: f64) -> bool {
        let Some(point) = self.scene_point(x, y) else {
            return false;
        };
        let Some(session) = &mut self.session else {
            return false;
        };
        let hotspot = session.prototype_hotspot_at(point.x, point.y);
        session.set_now_ms(now as u64);
        session.dispatch_tap(point.x, point.y);
        hotspot
    }

    pub fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let Some(point) = self.scene_point(x, y) else {
            return false;
        };
        let Some(session) = &mut self.session else {
            return false;
        };
        let dx = dx / self.scale;
        let dy = dy / self.scale;
        session.scroll_prototype(point.x, point.y, dx, dy)
            || session.dispatch_wheel(point.x, point.y, -dx, -dy)
    }

    pub fn frame(&mut self, now: f64, force: bool, hints: bool) -> Option<f64> {
        let event = self.session.as_mut()?.pump(now as u64);
        let result = self.session.as_mut()?.reconcile(now as u64);
        if result.switched {
            self.update_viewport();
        }
        let session = self.session.as_ref()?;
        let animating = session.transition_active();
        let painting = force
            || event.needs_redraw
            || result.invalidation() != InvalidationKind::None
            || self.animating
            || animating;
        self.animating = animating;
        if painting {
            self.paint(now as u64, hints);
            if self.backend.drain_pending_decodes(16) > 0 {
                self.paint(now as u64, hints);
                return Some(now + 16.0);
            }
        }
        self.session
            .as_ref()?
            .next_wake_deadline_ms()
            .map(|deadline| deadline as f64)
    }
}

impl PrototypePlayer {
    fn restore_entry(&mut self, now: u64, path: Option<&str>) {
        if let Some(session) = &mut self.session {
            if let Some(path) = path.or(self.entry.as_deref()) {
                session.navigate_to_screen(path);
                session.reconcile(now);
            }
        }
        self.update_viewport();
    }

    fn update_viewport(&mut self) {
        let fit = self.fits_entire_frame();
        let Some(session) = &mut self.session else {
            return;
        };
        let Some((_, bounds)) = session.framed_root() else {
            return;
        };
        let (width, height) = self.backend.logical_size();
        let mut scale = width / bounds.size.x.max(1.0);
        if fit {
            scale = scale.min(height / bounds.size.y.max(1.0));
        }
        let shown_width = bounds.size.x * scale;
        let shown_height = (bounds.size.y * scale).min(height);
        let left = ((width - shown_width) / 2.0).max(0.0);
        let top = ((height - shown_height) / 2.0).max(0.0);
        session.set_prototype_viewport_height(shown_height / scale);
        self.region = Rect::xywh(left, top, shown_width, shown_height);
        self.origin = self.region.origin - bounds.origin * scale;
        self.scale = scale;
    }

    fn scene_point(&self, x: f32, y: f32) -> Option<Point2D> {
        let point = Point2D::new(x, y);
        self.region
            .contains(point)
            .then(|| (point - self.origin) / self.scale)
    }

    fn paint(&mut self, now: u64, hints: bool) {
        let Some(session) = &self.session else { return };
        let Some((root, _)) = session.framed_root() else {
            return;
        };
        let (width, height) = self.backend.logical_size();
        let fill = session
            .framed_root_fill()
            .filter(|color| color.a >= 1.0)
            .unwrap_or(Color::WHITE);
        self.backend.begin_frame();
        self.backend
            .fill_rect(Rect::xywh(0.0, 0.0, width, height), fill);
        session.paint_framed_animated(
            &mut self.backend,
            &root,
            self.region,
            self.origin,
            self.scale,
            None,
            None,
            now,
        );
        if hints && !self.animating {
            self.backend.save();
            self.backend.clip_rect(self.region);
            for rect in session.prototype_hotspots() {
                let bounds = Rect {
                    origin: self.origin + rect.origin * self.scale,
                    size: rect.size * self.scale,
                };
                self.backend
                    .fill_rect(bounds, Color::rgba_u8(36, 164, 240, 0.18));
                self.backend
                    .stroke_rect(bounds, Color::rgba_u8(36, 164, 240, 0.8), 1.0);
            }
            self.backend.restore();
        }
        self.backend.end_frame();
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn prepare_screens(document: &mut PenDocument) {
    let routes = document.routes.as_ref();
    for page in document.pages.iter_mut().flatten() {
        lift_screens(&mut page.children);
        if !page.children.iter().any(is_screen) {
            if let Some((path, _)) = routes
                .into_iter()
                .flat_map(|routes| &routes.routes)
                .find(|(_, route)| route.page_id == page.id)
            {
                if let Some(PenNode::Frame(frame)) = page
                    .children
                    .iter_mut()
                    .find(|node| matches!(node, PenNode::Frame(_)))
                {
                    frame.screen = Some(path.clone());
                }
            }
        }
    }
    lift_screens(&mut document.children);
}

fn is_screen(node: &PenNode) -> bool {
    matches!(node, PenNode::Frame(frame) if frame.screen.is_some())
}

fn lift_screens(nodes: &mut Vec<PenNode>) {
    fn lift(node: &mut PenNode, screens: &mut Vec<PenNode>) {
        node.base_mut().locked = None;
        if let Some(children) = node.children_mut() {
            for child in children.iter_mut() {
                lift(child, screens);
            }
            screens.extend(children.extract_if(.., |node| is_screen(node)));
        }
    }
    let mut screens = Vec::new();
    for node in nodes.iter_mut() {
        lift(node, &mut screens);
    }
    nodes.append(&mut screens);
}
