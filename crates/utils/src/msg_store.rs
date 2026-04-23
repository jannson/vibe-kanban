use std::{
    collections::VecDeque,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::response::sse::Event;
use futures::{StreamExt, TryStreamExt, future};
use serde_json::Value;
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_stream::wrappers::BroadcastStream;

use crate::{log_msg::LogMsg, stream_lines::LinesStreamExt};

// 100 MB Limit
const HISTORY_BYTES: usize = 100000 * 1024;

#[derive(Clone)]
struct StoredMsg {
    msg: Arc<LogMsg>,
    bytes: usize,
}

struct Inner {
    history: VecDeque<StoredMsg>,
    total_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct MsgStoreStats {
    pub history_len: usize,
    pub total_bytes: usize,
    pub retain_raw_history: bool,
    pub compact_mode_enabled: bool,
}

pub struct MsgStore {
    inner: RwLock<Inner>,
    sender: broadcast::Sender<LogMsg>,
    retain_raw_history: AtomicBool,
    compact_mode_enabled: AtomicBool,
}

impl Default for MsgStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MsgStore {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(10000);
        Self {
            inner: RwLock::new(Inner {
                history: VecDeque::with_capacity(32),
                total_bytes: 0,
            }),
            sender,
            retain_raw_history: AtomicBool::new(true),
            compact_mode_enabled: AtomicBool::new(false),
        }
    }

    pub fn push(&self, msg: LogMsg) {
        let _ = self.sender.send(msg.clone()); // live listeners
        if matches!(msg, LogMsg::Stdout(_) | LogMsg::Stderr(_))
            && !self.retain_raw_history.load(Ordering::Relaxed)
        {
            return;
        }

        let mut msg = msg;
        let mut bytes = msg.approx_bytes();

        let mut inner = self.inner.write().unwrap();
        if let Some(compacted) = compact_patch_history(&mut inner, &msg) {
            match compacted {
                CompactedPatch::DropNew => return,
                CompactedPatch::ReplaceWith(next_msg) => {
                    msg = next_msg;
                    bytes = msg.approx_bytes();
                }
            }
        }

        let msg = Arc::new(msg);
        while inner.total_bytes.saturating_add(bytes) > HISTORY_BYTES {
            if let Some(front) = inner.history.pop_front() {
                inner.total_bytes = inner.total_bytes.saturating_sub(front.bytes);
            } else {
                break;
            }
        }
        inner.history.push_back(StoredMsg { msg, bytes });
        inner.total_bytes = inner.total_bytes.saturating_add(bytes);

        let memory_trace_enabled = std::env::var("VIBE_KANBAN_LOG_MEMORY_TRACE")
            .map(|v| v == "1")
            .unwrap_or(false);
        if memory_trace_enabled && inner.total_bytes >= (HISTORY_BYTES * 9 / 10) {
            tracing::warn!(
                target: "history",
                history_len = inner.history.len(),
                total_bytes = inner.total_bytes,
                retain_raw_history = self.retain_raw_history.load(Ordering::Relaxed),
                history_limit_bytes = HISTORY_BYTES,
                "MsgStore history is approaching its in-memory limit"
            );
        }
    }

    // Convenience
    pub fn push_stdout<S: Into<String>>(&self, s: S) {
        self.push(LogMsg::Stdout(s.into()));
    }

    pub fn push_stderr<S: Into<String>>(&self, s: S) {
        self.push(LogMsg::Stderr(s.into()));
    }
    pub fn push_patch(&self, patch: json_patch::Patch) {
        self.push(LogMsg::JsonPatch(patch));
    }

    pub fn push_session_id(&self, session_id: String) {
        self.push(LogMsg::SessionId(session_id));
    }

    pub fn push_finished(&self) {
        self.push(LogMsg::Finished);
    }

    pub fn disable_raw_history_retention(&self) {
        self.retain_raw_history.store(false, Ordering::Relaxed);
    }

    pub fn enable_raw_history_retention(&self) {
        self.retain_raw_history.store(true, Ordering::Relaxed);
    }

    pub fn enter_compact_mode(&self) {
        self.compact_mode_enabled.store(true, Ordering::Relaxed);
        self.disable_raw_history_retention();
    }

    pub fn compact_mode_enabled(&self) -> bool {
        self.compact_mode_enabled.load(Ordering::Relaxed)
    }

    pub fn get_receiver(&self) -> broadcast::Receiver<LogMsg> {
        self.sender.subscribe()
    }

    pub fn stats(&self) -> MsgStoreStats {
        let inner = self.inner.read().unwrap();
        MsgStoreStats {
            history_len: inner.history.len(),
            total_bytes: inner.total_bytes,
            retain_raw_history: self.retain_raw_history.load(Ordering::Relaxed),
            compact_mode_enabled: self.compact_mode_enabled.load(Ordering::Relaxed),
        }
    }

    pub fn get_history(&self) -> Vec<LogMsg> {
        self.inner
            .read()
            .unwrap()
            .history
            .iter()
            .map(|s| (*s.msg).clone())
            .collect()
    }

    fn get_history_refs(&self) -> Vec<Arc<LogMsg>> {
        self.inner
            .read()
            .unwrap()
            .history
            .iter()
            .map(|s| Arc::clone(&s.msg))
            .collect()
    }

    /// History then live, as `LogMsg`.
    pub fn history_plus_stream(
        &self,
    ) -> futures::stream::BoxStream<'static, Result<LogMsg, std::io::Error>> {
        let (history, rx) = (self.get_history_refs(), self.get_receiver());

        let hist = futures::stream::iter(
            history
                .into_iter()
                .map(|msg| Ok::<_, std::io::Error>((*msg).clone())),
        );
        let live = BroadcastStream::new(rx)
            .filter_map(|res| async move { res.ok().map(Ok::<_, std::io::Error>) });

        Box::pin(hist.chain(live))
    }

    pub fn stdout_chunked_stream(
        &self,
    ) -> futures::stream::BoxStream<'static, Result<String, std::io::Error>> {
        self.history_plus_stream()
            .take_while(|res| future::ready(!matches!(res, Ok(LogMsg::Finished))))
            .filter_map(|res| async move {
                match res {
                    Ok(LogMsg::Stdout(s)) => Some(Ok(s)),
                    _ => None,
                }
            })
            .boxed()
    }

    pub fn stdout_lines_stream(
        &self,
    ) -> futures::stream::BoxStream<'static, std::io::Result<String>> {
        self.stdout_chunked_stream().lines()
    }

    pub fn stderr_chunked_stream(
        &self,
    ) -> futures::stream::BoxStream<'static, Result<String, std::io::Error>> {
        self.history_plus_stream()
            .take_while(|res| future::ready(!matches!(res, Ok(LogMsg::Finished))))
            .filter_map(|res| async move {
                match res {
                    Ok(LogMsg::Stderr(s)) => Some(Ok(s)),
                    _ => None,
                }
            })
            .boxed()
    }

    pub fn stderr_lines_stream(
        &self,
    ) -> futures::stream::BoxStream<'static, std::io::Result<String>> {
        self.stderr_chunked_stream().lines()
    }

    /// Same stream but mapped to `Event` for SSE handlers.
    pub fn sse_stream(&self) -> futures::stream::BoxStream<'static, Result<Event, std::io::Error>> {
        self.history_plus_stream()
            .map_ok(|m| m.to_sse_event())
            .boxed()
    }

    /// Forward a stream of typed log messages into this store.
    pub fn spawn_forwarder<S, E>(self: Arc<Self>, stream: S) -> JoinHandle<()>
    where
        S: futures::Stream<Item = Result<LogMsg, E>> + Send + 'static,
        E: std::fmt::Display + Send + 'static,
    {
        tokio::spawn(async move {
            tokio::pin!(stream);

            while let Some(next) = stream.next().await {
                match next {
                    Ok(msg) => self.push(msg),
                    Err(e) => self.push(LogMsg::Stderr(format!("stream error: {e}"))),
                }
            }
        })
    }
}

enum PatchOp {
    Add,
    Replace,
    Remove,
}

struct SinglePatchMeta {
    path: String,
    op: PatchOp,
    value: Option<Value>,
}

enum CompactedPatch {
    DropNew,
    ReplaceWith(LogMsg),
}

fn compact_patch_history(inner: &mut Inner, msg: &LogMsg) -> Option<CompactedPatch> {
    let meta = single_patch_meta(msg)?;
    if meta.path.ends_with("/-") {
        return None;
    }

    let mut matched_indices = Vec::new();
    let mut saw_add = false;
    for (idx, stored) in inner.history.iter().enumerate() {
        let Some(stored_meta) = single_patch_meta(stored.msg.as_ref()) else {
            continue;
        };
        if stored_meta.path != meta.path {
            continue;
        }
        if matches!(stored_meta.op, PatchOp::Add) {
            saw_add = true;
        }
        matched_indices.push(idx);
    }

    if matched_indices.is_empty() {
        return None;
    }

    for idx in matched_indices.into_iter().rev() {
        if let Some(removed) = inner.history.remove(idx) {
            inner.total_bytes = inner.total_bytes.saturating_sub(removed.bytes);
        }
    }

    match meta.op {
        PatchOp::Remove => Some(CompactedPatch::DropNew),
        PatchOp::Add | PatchOp::Replace => {
            let op = if saw_add { "add" } else { "replace" };
            let value = meta.value?;
            Some(CompactedPatch::ReplaceWith(LogMsg::JsonPatch(
                single_patch_from_parts(op, &meta.path, value),
            )))
        }
    }
}

fn single_patch_meta(msg: &LogMsg) -> Option<SinglePatchMeta> {
    let LogMsg::JsonPatch(patch) = msg else {
        return None;
    };

    let value = serde_json::to_value(patch).ok()?;
    let ops = value.as_array()?;
    if ops.len() != 1 {
        return None;
    }
    let op = ops.first()?;
    let op_name = op.get("op")?.as_str()?;
    let path = op.get("path")?.as_str()?.to_string();
    let value = op.get("value").cloned();

    let op = match op_name {
        "add" => PatchOp::Add,
        "replace" => PatchOp::Replace,
        "remove" => PatchOp::Remove,
        _ => return None,
    };

    Some(SinglePatchMeta { path, op, value })
}

fn single_patch_from_parts(op: &str, path: &str, value: Value) -> json_patch::Patch {
    serde_json::from_value(serde_json::json!([{
        "op": op,
        "path": path,
        "value": value,
    }]))
    .expect("single patch should deserialize")
}

#[cfg(test)]
mod tests {
    use super::MsgStore;
    use crate::log_msg::LogMsg;
    use json_patch::Patch;
    use serde_json::json;

    fn replace_patch(path: &str, value: &str) -> Patch {
        serde_json::from_value(json!([{
            "op": "replace",
            "path": path,
            "value": {
                "type": "STDOUT",
                "content": value,
            }
        }]))
        .unwrap()
    }

    #[test]
    fn disabling_raw_history_retention_stops_storing_new_raw_logs() {
        let store = MsgStore::new();
        store.push_stdout("before");

        store.disable_raw_history_retention();
        store.push_stdout("after-stdout");
        store.push_stderr("after-stderr");
        store.push(LogMsg::SessionId("session-1".to_string()));
        store.push_finished();

        let history = store.get_history();

        assert!(history.iter().any(|msg| matches!(
            msg,
            LogMsg::Stdout(content) if content == "before"
        )));
        assert!(!history.iter().any(|msg| matches!(
            msg,
            LogMsg::Stdout(content) if content == "after-stdout"
        )));
        assert!(!history.iter().any(|msg| matches!(
            msg,
            LogMsg::Stderr(content) if content == "after-stderr"
        )));
        assert!(history.iter().any(|msg| matches!(
            msg,
            LogMsg::SessionId(content) if content == "session-1"
        )));
        assert!(history.iter().any(|msg| matches!(msg, LogMsg::Finished)));
    }

    #[test]
    fn entering_compact_mode_disables_raw_history_and_sets_flag() {
        let store = MsgStore::new();
        assert!(!store.compact_mode_enabled());
        assert!(store.stats().retain_raw_history);

        store.enter_compact_mode();

        let stats = store.stats();
        assert!(store.compact_mode_enabled());
        assert!(stats.compact_mode_enabled);
        assert!(!stats.retain_raw_history);

        store.push_stdout("skipped-after-compact");
        let history = store.get_history();
        assert!(!history.iter().any(|msg| matches!(
            msg,
            LogMsg::Stdout(content) if content == "skipped-after-compact"
        )));
    }

    #[test]
    fn re_enabling_raw_history_retention_resumes_storage() {
        let store = MsgStore::new();
        store.disable_raw_history_retention();
        store.push_stdout("skipped");

        store.enable_raw_history_retention();
        store.push_stdout("stored-again");

        let history = store.get_history();
        assert!(!history.iter().any(|msg| matches!(
            msg,
            LogMsg::Stdout(content) if content == "skipped"
        )));
        assert!(history.iter().any(|msg| matches!(
            msg,
            LogMsg::Stdout(content) if content == "stored-again"
        )));
    }

    #[test]
    fn replacing_same_patch_path_compacts_history() {
        let store = MsgStore::new();

        store.push(LogMsg::JsonPatch(replace_patch("/entries/0", "first")));
        let first_stats = store.stats();

        store.push(LogMsg::JsonPatch(replace_patch(
            "/entries/0",
            &"x".repeat(32_000),
        )));
        let second_stats = store.stats();

        store.push(LogMsg::JsonPatch(replace_patch(
            "/entries/0",
            &"y".repeat(32_000),
        )));
        let third_stats = store.stats();

        assert_eq!(first_stats.history_len, 1);
        assert_eq!(second_stats.history_len, 1);
        assert_eq!(third_stats.history_len, 1);
        assert!(third_stats.total_bytes < first_stats.total_bytes + second_stats.total_bytes);
    }

    #[test]
    fn append_path_patches_are_not_compacted() {
        let store = MsgStore::new();

        let append_first: Patch = serde_json::from_value(json!([{
            "op": "add",
            "path": "/entries/-",
            "value": {
                "type": "NORMALIZED_ENTRY",
                "content": {
                    "timestamp": null,
                    "entry_type": { "type": "system_message" },
                    "content": "first",
                    "metadata": null
                }
            }
        }]))
        .unwrap();

        let append_second: Patch = serde_json::from_value(json!([{
            "op": "add",
            "path": "/entries/-",
            "value": {
                "type": "NORMALIZED_ENTRY",
                "content": {
                    "timestamp": null,
                    "entry_type": { "type": "system_message" },
                    "content": "second",
                    "metadata": null
                }
            }
        }]))
        .unwrap();

        store.push(LogMsg::JsonPatch(append_first));
        store.push(LogMsg::JsonPatch(append_second));

        let stats = store.stats();
        assert_eq!(stats.history_len, 2);
    }
}
