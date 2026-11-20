use crate::binding_overlay::translate_for_scroll;
use crate::PreviewSession;
use jian_ops_schema::node::PenNode;
use op_editor_core::PenNodeExt;
use op_editor_ui::layout_scene::{LayoutScene, SceneNode};
use op_editor_ui::Rect;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Default)]
struct Axes {
    horizontal: bool,
    vertical: bool,
}

#[derive(Default)]
pub(crate) struct PrototypeViewport {
    overflow: HashMap<String, Axes>,
    pinned: BTreeSet<String>,
    sticky: HashSet<String>,
    hotspots: HashSet<String>,
    offsets: HashMap<String, (f32, f32)>,
    root_height: f32,
    bottom_bar: Option<String>,
}

impl PreviewSession {
    pub fn enable_prototype_viewport(&mut self) {
        let mut viewport = PrototypeViewport::default();
        viewport.read_nodes(&self.layout_doc.children);
        for page in self.layout_doc.pages.iter().flatten() {
            viewport.read_nodes(&page.children);
        }
        self.prototype_viewport = Some(viewport);
    }

    pub fn prototype_has_authored_viewport(&self) -> bool {
        self.prototype_viewport.as_ref().is_some_and(|viewport| {
            self.framed_root()
                .is_some_and(|(id, _)| viewport.overflow.contains_key(&id))
        })
    }

    pub fn set_prototype_viewport_height(&mut self, height: f32) {
        let bottom_bar = if self.prototype_has_authored_viewport() {
            None
        } else {
            self.pinned_nav_candidate(true).map(|(id, _)| id)
        };
        if let Some(viewport) = &mut self.prototype_viewport {
            viewport.root_height = height;
            viewport.bottom_bar = bottom_bar;
        }
    }

    pub fn scroll_prototype(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        if self.prototype_viewport.is_none() {
            return false;
        }
        let scene = self.overlay_runtime_state(&self.scene);
        let Some(root) = scene.active_page().and_then(|page| page.children.first()) else {
            return false;
        };
        let Some(authored) = self
            .scene
            .active_page()
            .and_then(|page| page.children.first())
        else {
            return false;
        };
        let mut chain = Vec::new();
        if !scroll_chain(root, x, y, &mut chain) {
            return false;
        }
        let viewport = self.prototype_viewport.as_mut().unwrap();
        for candidate in chain {
            let is_root = candidate.id == root.id;
            let Some(base) = find_node(authored, &candidate.id) else {
                continue;
            };
            let (max_x, max_y) = viewport.limits(base, candidate.bounds.size.y, is_root);
            let previous = viewport.offset(&candidate.id, (max_x, max_y));
            let next = (
                (previous.0 + dx).clamp(0.0, max_x),
                (previous.1 + dy).clamp(0.0, max_y),
            );
            if next != previous {
                viewport.offsets.insert(candidate.id.clone(), next);
                return true;
            }
        }
        false
    }

    pub fn prototype_hotspots(&self) -> Vec<Rect> {
        let Some(viewport) = &self.prototype_viewport else {
            return Vec::new();
        };
        if viewport.hotspots.is_empty() {
            return Vec::new();
        }
        let scene = self.overlay_runtime_state(&self.scene);
        let Some(root) = scene.active_page().and_then(|page| page.children.first()) else {
            return Vec::new();
        };
        let mut regions = Vec::new();
        collect_hotspots(root, root.bounds, &viewport.hotspots, &mut regions);
        regions
    }

    pub fn prototype_hotspot_at(&self, x: f32, y: f32) -> bool {
        let Some(viewport) = &self.prototype_viewport else {
            return false;
        };
        let Some((_, _, id)) = self.deepest_mapped_hit(x, y) else {
            return false;
        };
        let Some(document) = self.runtime.document.as_ref() else {
            return false;
        };
        let mut key = document.tree.by_id.get(&id).copied();
        while let Some(node) = key.and_then(|key| document.tree.nodes.get(key)) {
            if viewport.hotspots.contains(node.schema.id_str()) {
                return true;
            }
            key = node.parent;
        }
        false
    }
}

impl PrototypeViewport {
    fn read_nodes(&mut self, nodes: &[PenNode]) {
        for node in nodes {
            let id = node.id_str();
            if let Some(gestures) = node.gestures_and_semantics().0 {
                if let Some(value) = gestures
                    .extra
                    .get("prototypeOverflow")
                    .and_then(|value| value.as_str())
                {
                    self.overflow.insert(
                        id.to_owned(),
                        Axes {
                            horizontal: matches!(value, "horizontal" | "both"),
                            vertical: matches!(value, "vertical" | "both"),
                        },
                    );
                }
                if gestures
                    .extra
                    .get("prototypeSticky")
                    .and_then(|value| value.as_bool())
                    == Some(true)
                {
                    self.sticky.insert(id.to_owned());
                }
            }
            if node.base().pin == Some(true) {
                self.pinned.insert(id.to_owned());
            }
            if node
                .events()
                .and_then(|events| events.on_tap.as_ref())
                .is_some_and(|actions| !actions.is_empty())
            {
                self.hotspots.insert(id.to_owned());
            }
            if let Some(children) = node.children() {
                self.read_nodes(children);
            }
        }
    }

    fn pinned_at_root(&self, child: &SceneNode) -> bool {
        self.pinned.contains(&child.id) || self.bottom_bar.as_ref() == Some(&child.id)
    }

    fn pinned_footer_top(&self, root: &SceneNode) -> f32 {
        let bottom = root.bounds.origin.y + root.bounds.size.y;
        let center = root.bounds.origin.y + root.bounds.size.y / 2.0;
        let mut layers = root
            .children
            .iter()
            .filter(|child| !child.hidden && self.pinned.contains(&child.id))
            .map(|child| {
                let top = child.bounds.origin.y;
                (top, top + child.bounds.size.y)
            })
            .filter(|(top, end)| *top >= center && *end <= bottom + 1.0)
            .collect::<Vec<_>>();
        layers.sort_by(|left, right| right.1.total_cmp(&left.1));
        layers.into_iter().fold(bottom, |footer_top, (top, end)| {
            if end + 2.0 >= footer_top {
                footer_top.min(top)
            } else {
                footer_top
            }
        })
    }

    fn limits(&self, node: &SceneNode, height: f32, is_root: bool) -> (f32, f32) {
        let legacy = is_root && !self.overflow.contains_key(&node.id);
        let axes = self.overflow.get(&node.id).copied().unwrap_or(Axes {
            vertical: legacy,
            horizontal: false,
        });
        if !axes.vertical && !axes.horizontal {
            return (0.0, 0.0);
        }
        let right = node.bounds.origin.x + node.bounds.size.x;
        let bottom = node.bounds.origin.y + height;
        let frame_bottom = if legacy {
            node.bounds.origin.y + node.bounds.size.y
        } else {
            bottom
        };
        let (end_x, end_y) = node
            .children
            .iter()
            .filter(|child| {
                !child.hidden
                    && !self.pinned.contains(&child.id)
                    && (!is_root || !self.pinned_at_root(child))
            })
            .map(|child| self.content_end(child))
            .fold((right, frame_bottom), |(max_x, max_y), (x, y)| {
                (max_x.max(x), max_y.max(y))
            });
        (
            if axes.horizontal {
                (end_x - right).max(0.0)
            } else {
                0.0
            },
            if axes.vertical {
                (end_y - bottom).max(0.0)
            } else {
                0.0
            },
        )
    }

    fn content_end(&self, node: &SceneNode) -> (f32, f32) {
        let right = node.bounds.origin.x + node.bounds.size.x;
        let bottom = node.bounds.origin.y + node.bounds.size.y;
        if node.clip_content || self.overflow.contains_key(&node.id) {
            return (right, bottom);
        }
        node.children
            .iter()
            .filter(|child| !child.hidden && !self.pinned.contains(&child.id))
            .map(|child| self.content_end(child))
            .fold((right, bottom), |(max_x, max_y), (x, y)| {
                (max_x.max(x), max_y.max(y))
            })
    }

    fn offset(&self, id: &str, (max_x, max_y): (f32, f32)) -> (f32, f32) {
        let (x, y) = self.offsets.get(id).copied().unwrap_or_default();
        (x.min(max_x), y.min(max_y))
    }

    pub(crate) fn apply(&self, scene: &mut LayoutScene) {
        if let Some(root) = scene
            .pages
            .get_mut(scene.active_page_index)
            .and_then(|page| page.children.first_mut())
        {
            self.apply_node(root, true);
        }
    }

    fn apply_node(&self, node: &mut SceneNode, is_root: bool) {
        let original_height = node.bounds.size.y;
        let height = if is_root && self.root_height > 0.0 {
            self.root_height
        } else {
            original_height
        };
        let is_scroller = is_root || self.overflow.contains_key(&node.id);
        let (offset_x, offset_y) = self.offset(&node.id, self.limits(node, height, is_root));
        let node_top = node.bounds.origin.y;
        let footer_top =
            (is_root && height != original_height).then(|| self.pinned_footer_top(node));
        let no_pinned_descendants = BTreeSet::new();
        for child in &mut node.children {
            let pinned = if is_root {
                self.pinned_at_root(child)
            } else {
                self.pinned.contains(&child.id)
            };
            if pinned && is_root {
                let child_top = child.bounds.origin.y;
                let in_footer = footer_top.is_some_and(|top| {
                    child_top >= top
                        && child_top + child.bounds.size.y <= node_top + original_height + 1.0
                });
                if self.bottom_bar.as_ref() == Some(&child.id) || in_footer {
                    translate_for_scroll(
                        child,
                        0.0,
                        height - original_height,
                        &no_pinned_descendants,
                    );
                }
            } else if !pinned && (offset_x > 0.0 || offset_y > 0.0) {
                let y = if self.sticky.contains(&child.id) {
                    offset_y.min((child.bounds.origin.y - node_top).max(0.0))
                } else {
                    offset_y
                };
                translate_for_scroll(child, -offset_x, -y, &no_pinned_descendants);
            }
            self.apply_node(child, false);
        }
        if is_scroller {
            node.bounds.size.y = height;
            node.clip_content = true;
            node.children.sort_by_key(|child| {
                !(self.pinned.contains(&child.id)
                    || (is_root && self.bottom_bar.as_ref() == Some(&child.id))
                    || (self.sticky.contains(&child.id) && child.bounds.origin.y <= node_top))
            });
        }
    }
}

fn contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.origin.x
        && x < rect.origin.x + rect.size.x
        && y >= rect.origin.y
        && y < rect.origin.y + rect.size.y
}

fn scroll_chain<'scene>(
    node: &'scene SceneNode,
    x: f32,
    y: f32,
    output: &mut Vec<&'scene SceneNode>,
) -> bool {
    let inside = contains(node.bounds, x, y);
    if node.hidden || (node.clip_content && !inside) {
        return false;
    }
    let child_hit = node
        .children
        .iter()
        .any(|child| scroll_chain(child, x, y, output));
    if inside {
        output.push(node);
    }
    inside || child_hit
}

fn find_node<'scene>(node: &'scene SceneNode, id: &str) -> Option<&'scene SceneNode> {
    if node.id == id {
        Some(node)
    } else {
        node.children.iter().find_map(|child| find_node(child, id))
    }
}

fn intersection(first: Rect, second: Rect) -> Option<Rect> {
    let left = first.origin.x.max(second.origin.x);
    let top = first.origin.y.max(second.origin.y);
    let right = (first.origin.x + first.size.x).min(second.origin.x + second.size.x);
    let bottom = (first.origin.y + first.size.y).min(second.origin.y + second.size.y);
    (right > left && bottom > top).then(|| Rect::xywh(left, top, right - left, bottom - top))
}

fn collect_hotspots(node: &SceneNode, clip: Rect, ids: &HashSet<String>, output: &mut Vec<Rect>) {
    if node.hidden || output.len() >= 128 {
        return;
    }
    let bounds = intersection(node.bounds, clip);
    if ids.contains(&node.id) {
        output.extend(bounds);
    }
    let child_clip = if node.clip_content {
        bounds
    } else {
        Some(clip)
    };
    if let Some(clip) = child_clip {
        for child in &node.children {
            collect_hotspots(child, clip, ids, output);
        }
    }
}
