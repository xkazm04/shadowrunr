//! Session review for the console's Review page: the session list, one session's full record,
//! the line tagger (`tags.json`) and the operator's feedback log (`<data>/review/feedback.jsonl`).

use std::sync::Arc;

use shadowrunr_types::review::{
    FeedbackListParams, FeedbackSetParams, LineFeedback, ReviewSession, ReviewSessionParams,
    ReviewSessionRow, ReviewSessionsParams, ReviewTagParams, SessionTags,
};
use shadowrunr_types::ErrorCode;

use crate::rpc::RpcError;
use crate::server::AppState;

fn not_built() -> RpcError {
    RpcError::new(ErrorCode::Unavailable, "session review is not built yet")
}

pub async fn sessions(
    state: &Arc<AppState>,
    p: ReviewSessionsParams,
) -> Result<Vec<ReviewSessionRow>, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn session(
    state: &Arc<AppState>,
    p: ReviewSessionParams,
) -> Result<ReviewSession, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn feedback_set(
    state: &Arc<AppState>,
    p: FeedbackSetParams,
) -> Result<LineFeedback, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn feedback_list(
    state: &Arc<AppState>,
    p: FeedbackListParams,
) -> Result<Vec<LineFeedback>, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn tag(state: &Arc<AppState>, p: ReviewTagParams) -> Result<SessionTags, RpcError> {
    let _ = (state, p);
    Err(not_built())
}
