//! Small synchronous helpers shared by the VS Code bridge handlers.

use super::*;

pub(super) fn read_triple<C: RepaintContext>(inner: &Rc<RefCell<C>>) -> Option<(u64, u64, bool)> {
    let b = inner.try_borrow().ok()?;
    let s = b.host().editor_state();
    Some((s.document_generation(), s.document_revision(), s.is_dirty()))
}

fn studio_ai_busy<C: RepaintContext>(inner: &Rc<RefCell<C>>) -> Option<bool> {
    let context = inner.try_borrow().ok()?;
    Some(
        context
            .host()
            .editor_state()
            .chat
            .tabs()
            .iter()
            .any(op_editor_core::ChatState::has_streaming_turn),
    )
}

pub(super) fn install_studio_ai_observer<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let inner = inner.clone();
    let last = std::cell::Cell::new(false);
    let tick = Rc::new(move || {
        if let Some(busy) = studio_ai_busy(&inner) {
            if busy != last.replace(busy) {
                post_to_parent(
                    &serde_json::json!({"type":"op-dsh/ai-state","busy":busy}).to_string(),
                );
            }
        }
    });
    let _ = live_sync::start_interval(100, tick);
}

pub(super) fn sync_studio_snapshot<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    sync: &SharedSync,
    request_id: String,
    retries: u8,
) {
    let waiting = |id: &str| {
        post_to_parent(
            &serde_json::json!({"type":"op-dsh/snapshot-wait","requestId":id}).to_string(),
        );
    };
    if studio_ai_busy(inner) != Some(false) {
        waiting(&request_id);
        return;
    }
    let inner = inner.clone();
    let sync = sync.clone();
    let fallback_id = request_id.clone();
    let url = crate::daemon_base::daemon_url("/api/mcp/version");
    let callback = Rc::new(move |status: u16, body: String| {
        let version = (status == 200)
            .then(|| WebSyncClient::parse_version_probe(&body))
            .flatten();
        if studio_ai_busy(&inner) != Some(false) || version.is_none() {
            waiting(&request_id);
        } else if version.is_some_and(|version| version <= sync.borrow().client.last_version())
            || sync.borrow().gate.conflict().is_some()
        {
            capture_snapshot(&inner, &sync, request_id.clone());
        } else if retries > 0 {
            let (inner, sync, id) = (inner.clone(), sync.clone(), request_id.clone());
            schedule_once(300, move || {
                sync_studio_snapshot(&inner, &sync, id, retries - 1)
            });
        } else {
            waiting(&request_id);
        }
    });
    if !live_sync::get_with_status(&url, callback) {
        waiting(&fallback_id);
    }
}

/// Serialize the live document and editor metadata atomically.
pub(super) fn snapshot_state<C: RepaintContext>(
    inner: &Rc<RefCell<C>>,
) -> Option<BridgeDocumentSnapshot> {
    let b = inner.try_borrow().ok()?;
    BridgeDocumentSnapshot::capture(b.host().editor_state())
}

/// Try to claim the shared push single-flight latch. `true` when acquired.
pub(super) fn acquire_push_busy(sync: &SharedSync) -> bool {
    match sync.try_borrow_mut() {
        Ok(mut s) if !s.push_busy => {
            s.push_busy = true;
            true
        }
        _ => false,
    }
}

pub(super) fn release_push_busy(sync: &SharedSync) {
    if let Ok(mut s) = sync.try_borrow_mut() {
        s.push_busy = false;
    }
}

/// Post a codec string to the locked host origin (falling back to `*` only
/// before the origin is known — by which point every real reply is sent).
pub(crate) fn post_to_parent(json: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(parent) = window.parent().ok().flatten() else {
        return;
    };
    let target = BRIDGE_ORIGIN
        .with(|o| o.borrow().clone())
        .unwrap_or_else(|| "*".to_string());
    let _ = parent.post_message(&JsValue::from_str(json), &target);
}

/// One-shot `setTimeout`. `once_into_js` self-frees after firing.
pub(super) fn schedule_once<F: FnOnce() + 'static>(delay_ms: i32, f: F) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let cb = Closure::once_into_js(f);
    let _ =
        window.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), delay_ms);
}
