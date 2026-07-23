use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::error;

use crate::artifact_store::workspace_output_root;
use crate::{now_ms, AppState, COLLAB_COMMENT_HISTORY_DIR, MAX_COLLAB_COMMENT_HISTORY};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CollabClientEvent {
    pub(crate) kind: String,
    pub(crate) client_id: String,
    pub(crate) user_name: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) revision: Option<u64>,
    pub(crate) cursor_line: Option<usize>,
    pub(crate) cursor_column: Option<usize>,
    pub(crate) comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CollabEvent {
    pub(crate) room: String,
    pub(crate) kind: String,
    pub(crate) client_id: String,
    pub(crate) user_name: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) revision: Option<u64>,
    pub(crate) cursor_line: Option<usize>,
    pub(crate) cursor_column: Option<usize>,
    pub(crate) comment: Option<String>,
    pub(crate) at_ms: u64,
}

pub(crate) async fn collab_room_sender(
    state: &Arc<AppState>,
    room: &str,
) -> broadcast::Sender<CollabEvent> {
    {
        let rooms = state.collab_rooms.read().await;
        if let Some(sender) = rooms.get(room) {
            return sender.clone();
        }
    }

    let mut rooms = state.collab_rooms.write().await;
    rooms
        .entry(room.to_string())
        .or_insert_with(|| {
            let (sender, _) = broadcast::channel(128);
            sender
        })
        .clone()
}

pub(crate) async fn record_collab_comment(state: &Arc<AppState>, event: &CollabEvent) {
    if !is_collab_comment_event(event) {
        return;
    }

    let history = {
        let mut rooms = state.collab_comments.write().await;
        let history = rooms.entry(event.room.clone()).or_default();
        history.push(event.clone());
        trim_collab_comment_history(history);
        history.clone()
    };

    if let Err(err) = write_collab_comment_history(&state.workspace_root, &event.room, &history) {
        error!(room = %event.room, error = %err, "failed to persist collaboration comment history");
    }
}

pub(crate) async fn collab_comment_history(state: &Arc<AppState>, room: &str) -> Vec<CollabEvent> {
    if let Some(history) = state.collab_comments.read().await.get(room).cloned() {
        return history;
    }

    let history = read_collab_comment_history(&state.workspace_root, room);
    if history.is_empty() {
        return history;
    }

    state
        .collab_comments
        .write()
        .await
        .insert(room.to_string(), history.clone());
    history
}

fn trim_collab_comment_history(history: &mut Vec<CollabEvent>) {
    if history.len() > MAX_COLLAB_COMMENT_HISTORY {
        let overflow = history.len() - MAX_COLLAB_COMMENT_HISTORY;
        history.drain(0..overflow);
    }
}

fn collab_comment_history_path(workspace_root: &Path, room: &str) -> Option<PathBuf> {
    if !is_safe_collab_room(room) {
        return None;
    }
    Some(
        workspace_output_root(workspace_root)
            .ok()?
            .join(COLLAB_COMMENT_HISTORY_DIR)
            .join(format!("{room}.json")),
    )
}

fn read_collab_comment_history(workspace_root: &Path, room: &str) -> Vec<CollabEvent> {
    let Some(path) = collab_comment_history_path(workspace_root, room) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(mut history) = serde_json::from_str::<Vec<CollabEvent>>(&text) else {
        return Vec::new();
    };

    history.retain(|event| event.room == room && is_collab_comment_event(event));
    trim_collab_comment_history(&mut history);
    history
}

fn write_collab_comment_history(
    workspace_root: &Path,
    room: &str,
    history: &[CollabEvent],
) -> Result<(), String> {
    let Some(path) = collab_comment_history_path(workspace_root, room) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create comment history dir: {err}"))?;
    }
    let mut text = serde_json::to_string_pretty(history)
        .map_err(|err| format!("failed to serialize comment history: {err}"))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|err| format!("failed to write comment history: {err}"))
}

fn is_collab_comment_event(event: &CollabEvent) -> bool {
    event.kind == "comment"
        && event
            .comment
            .as_deref()
            .map(|comment| !comment.trim().is_empty())
            .unwrap_or(false)
}

pub(crate) fn build_collab_event(room: &str, request: CollabClientEvent) -> CollabEvent {
    CollabEvent {
        room: room.to_string(),
        kind: request.kind,
        client_id: request.client_id,
        user_name: request.user_name,
        content: request.content,
        revision: request.revision,
        cursor_line: request.cursor_line,
        cursor_column: request.cursor_column,
        comment: request.comment,
        at_ms: now_ms(),
    }
}

pub(crate) fn is_safe_collab_room(room: &str) -> bool {
    !room.is_empty()
        && room.len() <= 96
        && room
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}
