//! Building the engine stack from config, and adapting the fleet to the brain.

use std::sync::Arc;

use shadowrunr_brain::{FleetMode as BrainFleetMode, FleetProbe};
use shadowrunr_engine::{build_fleet, Fleet, FleetMode, FleetState, ScriptedEngine};
use shadowrunr_types::{EndReason, Engine, Op, StreamEvent, TsMs};

use crate::config::Config;

/// The engine every command uses, plus the concrete fleet when there is one.
#[derive(Clone)]
pub struct EngineHandle {
    pub engine: Arc<dyn Engine>,
    /// `None` for the scripted engine.
    pub fleet: Option<Arc<Fleet>>,
}

impl EngineHandle {
    /// The live fleet from `[engine]`, or the scripted engine when `scripted` is set on the
    /// command line or in the config.
    pub fn build(cfg: &Config, scripted: bool) -> Self {
        if scripted || cfg.engine.scripted {
            return Self::scripted();
        }
        let fleet = build_fleet(&cfg.engine);
        Self {
            engine: fleet.clone(),
            fleet: Some(fleet),
        }
    }

    pub fn scripted() -> Self {
        Self {
            engine: Arc::new(ScriptedEngine),
            fleet: None,
        }
    }

    pub fn is_scripted(&self) -> bool {
        self.fleet.is_none()
    }

    /// The fleet as a [`FleetProbe`] for `Brain::with_fleet_probe`.
    pub fn probe(&self) -> Option<Arc<dyn FleetProbe>> {
        self.fleet
            .as_ref()
            .map(|f| Arc::new(FleetStateProbe::new(f.clone())) as Arc<dyn FleetProbe>)
    }
}

/// Maps the engine's [`FleetState`] onto the brain's fleet-loss policy.
pub struct FleetStateProbe {
    read: Box<dyn Fn() -> FleetState + Send + Sync>,
}

impl FleetStateProbe {
    pub fn new(fleet: Arc<Fleet>) -> Self {
        Self {
            read: Box::new(move || fleet.fleet_state()),
        }
    }

    /// From any state source (tests).
    pub fn from_fn(read: impl Fn() -> FleetState + Send + Sync + 'static) -> Self {
        Self {
            read: Box::new(read),
        }
    }
}

pub fn brain_mode(mode: FleetMode) -> BrainFleetMode {
    match mode {
        FleetMode::Primary => BrainFleetMode::Healthy,
        FleetMode::Degraded => BrainFleetMode::Degraded,
        FleetMode::Down => BrainFleetMode::Down,
    }
}

impl FleetProbe for FleetStateProbe {
    fn mode(&self) -> BrainFleetMode {
        brain_mode((self.read)().mode)
    }
}

/// For a live (non-sim) stream: when the brain's beat contains a `SignOff`, the stream ends
/// with `StreamEnd { reason: fleet_lost }`. Returns the event to append, if any.
pub fn fleet_lost_end(events: &[StreamEvent], now_ms: TsMs) -> Option<StreamEvent> {
    events
        .iter()
        .any(|e| {
            matches!(
                e,
                StreamEvent::Op {
                    op: Op::SignOff { .. },
                    ..
                }
            )
        })
        .then_some(StreamEvent::StreamEnd {
            ts_ms: now_ms,
            reason: EndReason::FleetLost,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn probe_maps_fleet_modes() {
        let state = Arc::new(Mutex::new(FleetState {
            mode: FleetMode::Primary,
            since_ms: 0,
        }));
        let s = state.clone();
        let probe = FleetStateProbe::from_fn(move || *s.lock().unwrap());
        assert_eq!(probe.mode(), BrainFleetMode::Healthy);
        state.lock().unwrap().mode = FleetMode::Degraded;
        assert_eq!(probe.mode(), BrainFleetMode::Degraded);
        state.lock().unwrap().mode = FleetMode::Down;
        assert_eq!(probe.mode(), BrainFleetMode::Down);
    }

    #[test]
    fn sign_off_ends_with_fleet_lost() {
        let beat = vec![StreamEvent::Op {
            ts_ms: 5,
            op: Op::SignOff {
                reason: "fleet".into(),
            },
        }];
        match fleet_lost_end(&beat, 9) {
            Some(StreamEvent::StreamEnd { ts_ms, reason }) => {
                assert_eq!(ts_ms, 9);
                assert_eq!(reason, EndReason::FleetLost);
            }
            other => panic!("{other:?}"),
        }
        assert!(fleet_lost_end(&[], 9).is_none());
    }
}
