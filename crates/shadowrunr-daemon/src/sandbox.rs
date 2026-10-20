//! `sandbox.start` / `sandbox.stop` / `sandbox.status`: the daemon hosts a wall-clock
//! session ([`shadowrunr_sim::sandbox::run_sandbox`]), broadcasts its events on `/events`,
//! records stage acks and judges the session when it ends.

use std::sync::Arc;

use shadowrunr_types::sandbox::{
    SandboxIdParams, SandboxStartParams, SandboxStarted, SandboxStatus, SandboxStatusParams,
};
use shadowrunr_types::ErrorCode;

use crate::rpc::RpcError;
use crate::server::AppState;

fn not_built() -> RpcError {
    RpcError::new(ErrorCode::Unavailable, "sandbox sessions are not built yet")
}

pub async fn start(
    state: Arc<AppState>,
    p: SandboxStartParams,
) -> Result<SandboxStarted, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn stop(state: &Arc<AppState>, p: SandboxIdParams) -> Result<SandboxStatus, RpcError> {
    let _ = (state, p);
    Err(not_built())
}

pub async fn status(
    state: &Arc<AppState>,
    p: SandboxStatusParams,
) -> Result<SandboxStatus, RpcError> {
    let _ = (state, p);
    Err(not_built())
}
