//! Stream prep: the planner writes a stream's cold open, premise deck and connector lines
//! before the stream (`prep.generate`), mechanically checked and staged under
//! `<data>/prep/<prep_id>/prep.json`; the owner approves one (`prep.approve`) and streams use the
//! newest approved prep.

use std::sync::Arc;

use shadowrunr_types::prep::{
    PrepApproveParams, PrepGenerateParams, PrepIdParams, PrepSummary, StreamPrep,
};
use shadowrunr_types::ErrorCode;

use crate::rpc::RpcError;
use crate::server::AppState;

fn not_built() -> RpcError {
    RpcError::new(ErrorCode::Unavailable, "stream prep is not built yet")
}

pub async fn generate(state: Arc<AppState>, p: PrepGenerateParams) -> Result<StreamPrep, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn list(state: &Arc<AppState>) -> Result<Vec<PrepSummary>, RpcError> {
    let _ = state;
    Err(not_built())
}

pub async fn get(state: &Arc<AppState>, p: PrepIdParams) -> Result<StreamPrep, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn approve(state: &Arc<AppState>, p: PrepApproveParams) -> Result<StreamPrep, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn retire(state: &Arc<AppState>, p: PrepIdParams) -> Result<StreamPrep, RpcError> {
    let _ = (state, p);
    Err(not_built())
}
