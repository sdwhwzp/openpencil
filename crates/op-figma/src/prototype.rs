use crate::kiwi::FigValue;
use crate::tree::{guid_to_string, TreeNode};
use jian_ops_schema::events::{Action, EventHandlers};
use jian_ops_schema::gestures::GestureOverrides;
use jian_ops_schema::node::PenNode;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Default)]
pub(crate) struct PrototypeImport {
    paths: HashMap<String, String>,
    flows: HashMap<String, String>,
}

enum Click {
    Navigate(String),
    Back,
}

fn tap_action(action: &FigValue) -> Option<Click> {
    match action.get_str("connectionType") {
        Some("BACK") => Some(Click::Back),
        Some("INTERNAL_NODE") | None
            if matches!(action.get_str("navigationType"), Some("NAVIGATE") | None)
                && action.get_bool("destinationIsOverlay") != Some(true) =>
        {
            Some(Click::Navigate(guid_to_string(
                action.get("transitionNodeID")?,
            )?))
        }
        _ => None,
    }
}

fn click_actions(source: &FigValue) -> impl Iterator<Item = Click> + '_ {
    let modern = source
        .get_array("prototypeInteractions")
        .into_iter()
        .flatten()
        .filter(|event| {
            event.get_bool("isDeleted") != Some(true)
                && event
                    .get("event")
                    .and_then(|trigger| trigger.get_str("interactionType"))
                    == Some("ON_CLICK")
        });
    modern
        .flat_map(|event| event.get_array("actions").into_iter().flatten())
        .filter_map(tap_action)
        .chain(
            (source.get_str("interactionType") == Some("ON_CLICK"))
                .then(|| tap_action(source))
                .flatten(),
        )
}

fn node_id(source: &FigValue) -> Option<String> {
    source.get("guid").and_then(guid_to_string)
}

fn visible(source: &FigValue) -> bool {
    source.get_bool("visible") != Some(false)
}

fn scroll_direction(source: &FigValue) -> Option<&'static str> {
    match source.get_str("scrollDirection") {
        Some("VERTICAL") => Some("vertical"),
        Some("HORIZONTAL") => Some("horizontal"),
        Some("BOTH") => Some("both"),
        _ => None,
    }
}

fn collect_frames<'tree>(parent: &'tree TreeNode, frames: &mut Vec<(&'tree TreeNode, String)>) {
    for child in &parent.children {
        if !visible(&child.figma) {
            continue;
        }
        match child.figma.get_str("type") {
            Some("SECTION") => collect_frames(child, frames),
            Some("FRAME") => {
                if let Some(id) = node_id(&child.figma) {
                    frames.push((child, id));
                }
            }
            _ => {}
        }
    }
}

fn collect_destinations(
    node: &TreeNode,
    screens: &HashSet<&str>,
    included: &mut HashSet<String>,
) -> bool {
    if !visible(&node.figma) {
        return false;
    }
    let mut linked = scroll_direction(&node.figma).is_some();
    for action in click_actions(&node.figma) {
        match action {
            Click::Navigate(target) if screens.contains(target.as_str()) => {
                included.insert(target);
                linked = true;
            }
            Click::Back => linked = true,
            _ => {}
        }
    }
    for child in &node.children {
        linked |= collect_destinations(child, screens, included);
    }
    linked
}

impl PrototypeImport {
    pub(crate) fn from_pages(pages: &[&TreeNode]) -> Self {
        let mut frames = Vec::new();
        let mut starts = Vec::new();
        for page in pages {
            let first = frames.len();
            collect_frames(page, &mut frames);
            let legacy_start = page
                .figma
                .get("prototypeStartNodeID")
                .and_then(guid_to_string);
            let mut page_starts = frames[first..]
                .iter()
                .filter_map(|(frame, id)| {
                    let point = frame.figma.get("prototypeStartingPoint");
                    if point.is_none() && legacy_start.as_ref() != Some(id) {
                        return None;
                    }
                    let position = point
                        .and_then(|value| value.get_str("position"))
                        .unwrap_or("");
                    let name = point
                        .and_then(|value| value.get_str("name"))
                        .or_else(|| frame.figma.get_str("name"))
                        .unwrap_or("Flow");
                    Some((position, id.clone(), name.to_owned()))
                })
                .collect::<Vec<_>>();
            page_starts.sort_by_key(|(position, _, _)| *position);
            starts.extend(page_starts);
        }
        let screens: HashSet<_> = frames.iter().map(|(_, id)| id.as_str()).collect();
        let mut included: HashSet<_> = starts.iter().map(|(_, id, _)| id.clone()).collect();
        for (frame, id) in &frames {
            if collect_destinations(frame, &screens, &mut included) {
                included.insert(id.clone());
            }
        }
        let entry = starts.first().map(|(_, id, _)| id).or_else(|| {
            frames
                .iter()
                .map(|(_, id)| id)
                .find(|id| included.contains(*id))
        });
        let paths = included
            .iter()
            .map(|id| {
                let path = if entry == Some(id) {
                    "/".to_owned()
                } else {
                    format!("/figma/{}", id.replace(':', "-"))
                };
                (id.clone(), path)
            })
            .collect();
        let flows = starts.into_iter().map(|(_, id, name)| (id, name)).collect();
        Self { paths, flows }
    }

    fn action(&self, source: &FigValue) -> Option<Action> {
        click_actions(source).find_map(|click| {
            let (name, body) = match click {
                Click::Back if !self.paths.is_empty() => ("pop", Value::Bool(true)),
                Click::Back => return None,
                Click::Navigate(target) => {
                    let route = self.paths.get(&target)?;
                    (
                        "push",
                        Value::String(Value::String(route.clone()).to_string()),
                    )
                }
            };
            Some(Action(BTreeMap::from([(name.to_owned(), body)])))
        })
    }

    pub(crate) fn keeps_layer(&self, source: &FigValue) -> bool {
        visible(source)
            && (source.get_f64("opacity").is_none_or(|alpha| alpha > 0.0)
                || self.action(source).is_some())
    }

    pub(crate) fn apply(&self, source: &FigValue, node: &mut PenNode) {
        let id = node_id(source);
        let path = id.as_ref().and_then(|id| self.paths.get(id));
        let flow = id.as_ref().and_then(|id| self.flows.get(id));
        let overflow = scroll_direction(source).or_else(|| path.map(|_| "none"));
        let position = source.get_str("scrollBehavior");
        let fixed = position == Some("FIXED_WHEN_CHILD_OF_SCROLLING_FRAME");
        let sticky = position == Some("STICKY_SCROLLS");
        let action = self.action(source);
        if path.is_none() && overflow.is_none() && !fixed && !sticky && action.is_none() {
            return;
        }

        if let (Some(path), PenNode::Frame(frame)) = (path, &mut *node) {
            frame.screen = Some(path.clone());
        }
        let (base, events, gestures) = match node {
            PenNode::Frame(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Group(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Rectangle(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Ellipse(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Path(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Line(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Text(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            PenNode::Ref(node) => (&mut node.base, &mut node.events, &mut node.gestures),
            _ => return,
        };
        if fixed {
            base.pin = Some(true);
        }
        if let Some(action) = action {
            events.get_or_insert_with(EventHandlers::default).on_tap = Some(vec![action]);
        }
        if overflow.is_some() || flow.is_some() || sticky {
            let extra = &mut gestures.get_or_insert_with(GestureOverrides::default).extra;
            if let Some(overflow) = overflow {
                extra.insert("prototypeOverflow".to_owned(), Value::from(overflow));
            }
            if let Some(name) = flow {
                extra.insert("prototypeFlow".to_owned(), Value::from(name.as_str()));
            }
            if sticky {
                extra.insert("prototypeSticky".to_owned(), Value::Bool(true));
            }
        }
    }
}
