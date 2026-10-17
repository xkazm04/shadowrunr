//! Segment portfolio lifecycle: `pilot -> trial -> regular -> flagship`, and `retired`.
//!
//! One consistent rule (it replaces the digest's mixed thresholds, including the stray -0.8):
//! - **promote** when the mean of the last 3 SES is >= +0.4;
//! - **retire** when SES <= -0.5 for 3 consecutive runs (retire wins over promote);
//! - **flagship** is the top regular, and only the owner can make it so: the candidate is
//!   flagged `pending_owner` until [`approve_flagship`].
//!
//! All three numbers are assumptions until calibrated over Season 0.

use serde::{Deserialize, Serialize};
use shadowrunr_types::{SegmentId, SegmentStatus};

use crate::SegmentError;

pub const SES_WINDOW: usize = 3;
pub const PROMOTE_MEAN_SES: f64 = 0.4;
pub const RETIRE_SES: f64 = -0.5;
/// Tolerance so a boundary value (for example three runs at exactly 0.4) is not lost to
/// floating-point rounding.
const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleTrigger {
    Promote,
    Retire,
    OwnerApprovedFlagship,
    /// The flagship slot went to another segment.
    Demote,
}

use LifecycleTrigger as T;
use SegmentStatus as S;

/// The lifecycle as a transition table: (from, trigger, to).
pub const LIFECYCLE: [(SegmentStatus, LifecycleTrigger, SegmentStatus); 8] = [
    (S::Pilot, T::Promote, S::Trial),
    (S::Trial, T::Promote, S::Regular),
    (S::Regular, T::OwnerApprovedFlagship, S::Flagship),
    (S::Flagship, T::Demote, S::Regular),
    (S::Pilot, T::Retire, S::Retired),
    (S::Trial, T::Retire, S::Retired),
    (S::Regular, T::Retire, S::Retired),
    (S::Flagship, T::Retire, S::Retired),
];

/// Apply a trigger through [`LIFECYCLE`]; `None` when the table has no row.
pub fn apply(current: SegmentStatus, trigger: LifecycleTrigger) -> Option<SegmentStatus> {
    LIFECYCLE
        .iter()
        .find(|(from, t, _)| *from == current && *t == trigger)
        .map(|(_, _, to)| *to)
}

/// The trigger SES history calls for, if any. `history` is oldest first.
pub fn ses_trigger(history: &[f64]) -> Option<LifecycleTrigger> {
    if history.len() < SES_WINDOW {
        return None;
    }
    let last = &history[history.len() - SES_WINDOW..];
    if last.iter().all(|s| *s <= RETIRE_SES + EPS) {
        return Some(T::Retire);
    }
    let mean = last.iter().sum::<f64>() / SES_WINDOW as f64;
    (mean >= PROMOTE_MEAN_SES - EPS).then_some(T::Promote)
}

/// Next status from SES history (oldest first; pass the runs since the last status change).
/// Regular never becomes Flagship here: that needs the owner (see [`flag_flagship_candidate`]).
pub fn next_status(history: &[f64], current: SegmentStatus) -> SegmentStatus {
    ses_trigger(history)
        .and_then(|t| apply(current, t))
        .unwrap_or(current)
}

fn recent_mean(history: &[f64]) -> Option<f64> {
    (history.len() >= SES_WINDOW)
        .then(|| history[history.len() - SES_WINDOW..].iter().sum::<f64>() / SES_WINDOW as f64)
}

/// One row of the segment portfolio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioEntry {
    pub segment: SegmentId,
    pub status: SegmentStatus,
    /// SES per run, oldest first.
    pub history: Vec<f64>,
    /// Set on the flagship candidate until the owner approves it.
    #[serde(default)]
    pub pending_owner: bool,
}

/// Flag the top regular as flagship candidate (`pending_owner = true`, all others cleared).
/// A candidate must meet the promote bar and beat the current flagship's recent mean.
pub fn flag_flagship_candidate(entries: &mut [PortfolioEntry]) -> Option<SegmentId> {
    let flagship_mean = entries
        .iter()
        .filter(|e| e.status == S::Flagship)
        .filter_map(|e| recent_mean(&e.history))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut best: Option<(usize, f64)> = None;
    for (i, e) in entries.iter().enumerate() {
        if e.status != S::Regular {
            continue;
        }
        let Some(m) = recent_mean(&e.history) else {
            continue;
        };
        if m >= PROMOTE_MEAN_SES - EPS && m > flagship_mean && best.is_none_or(|(_, b)| m > b) {
            best = Some((i, m));
        }
    }
    for e in entries.iter_mut() {
        e.pending_owner = false;
    }
    best.map(|(i, _)| {
        entries[i].pending_owner = true;
        entries[i].segment
    })
}

/// The owner approves the pending flagship candidate; the old flagship is demoted.
pub fn approve_flagship(
    entries: &mut [PortfolioEntry],
    segment: SegmentId,
) -> Result<(), SegmentError> {
    let idx = entries
        .iter()
        .position(|e| e.segment == segment && e.pending_owner)
        .ok_or_else(|| SegmentError::Config {
            detail: format!("{segment:?} is not a pending flagship candidate"),
        })?;
    let promoted = apply(entries[idx].status, T::OwnerApprovedFlagship).ok_or_else(|| {
        SegmentError::Config {
            detail: format!("{segment:?} is not regular"),
        }
    })?;
    for e in entries.iter_mut().filter(|e| e.status == S::Flagship) {
        e.status = apply(S::Flagship, T::Demote).unwrap_or(S::Regular);
    }
    entries[idx].status = promoted;
    entries[idx].pending_owner = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_status_table() {
        let cases: &[(&[f64], SegmentStatus, SegmentStatus)] = &[
            // Too little history: no change.
            (&[], S::Pilot, S::Pilot),
            (&[2.0, 2.0], S::Pilot, S::Pilot),
            (&[-1.0, -1.0], S::Trial, S::Trial),
            // Promote at mean >= 0.4, exactly on the boundary too.
            (&[0.4, 0.4, 0.4], S::Pilot, S::Trial),
            (&[0.0, 0.6, 0.6], S::Pilot, S::Trial),
            (&[0.4, 0.4, 0.39], S::Pilot, S::Pilot),
            (&[0.5, 0.5, 0.5], S::Trial, S::Regular),
            // Only the last 3 count.
            (&[-3.0, 0.5, 0.5, 0.5], S::Trial, S::Regular),
            (&[3.0, 3.0, 0.0, 0.0, 0.0], S::Trial, S::Trial),
            // Regular never self-promotes to flagship.
            (&[2.0, 2.0, 2.0], S::Regular, S::Regular),
            (&[2.0, 2.0, 2.0], S::Flagship, S::Flagship),
            // Retire at <= -0.5 for 3 consecutive runs, from any live stage.
            (&[-0.5, -0.5, -0.5], S::Pilot, S::Retired),
            (&[-0.6, -0.9, -0.5], S::Trial, S::Retired),
            (&[-1.0, -1.0, -1.0], S::Regular, S::Retired),
            (&[-1.0, -1.0, -1.0], S::Flagship, S::Retired),
            // Not consecutive: no retirement.
            (&[-1.0, -0.49, -1.0], S::Regular, S::Regular),
            (&[-1.0, -1.0, -1.0, 0.0], S::Regular, S::Regular),
            // Terminal and descoped statuses never move.
            (&[2.0, 2.0, 2.0], S::Retired, S::Retired),
            (&[-1.0, -1.0, -1.0], S::Descoped, S::Descoped),
            (&[2.0, 2.0, 2.0], S::Descoped, S::Descoped),
        ];
        for (history, current, want) in cases {
            assert_eq!(
                next_status(history, *current),
                *want,
                "{history:?} from {current:?}"
            );
        }
    }

    fn entry(segment: SegmentId, status: SegmentStatus, history: &[f64]) -> PortfolioEntry {
        PortfolioEntry {
            segment,
            status,
            history: history.to_vec(),
            pending_owner: false,
        }
    }

    #[test]
    fn flagship_needs_owner() {
        let mut p = vec![
            entry(
                SegmentId::Seg01ImprovisedRoom,
                S::Flagship,
                &[0.5, 0.5, 0.5],
            ),
            entry(SegmentId::Seg02aHexmap, S::Regular, &[0.9, 0.9, 0.9]),
        ];
        assert_eq!(
            flag_flagship_candidate(&mut p),
            Some(SegmentId::Seg02aHexmap)
        );
        assert!(p[1].pending_owner);
        assert_eq!(p[1].status, S::Regular, "still regular until approved");
        approve_flagship(&mut p, SegmentId::Seg02aHexmap).unwrap();
        assert_eq!(p[1].status, S::Flagship);
        assert_eq!(p[0].status, S::Regular, "old flagship demoted");
        assert!(!p[1].pending_owner);
        // Approving something that is not pending fails.
        assert!(approve_flagship(&mut p, SegmentId::Seg01ImprovisedRoom).is_err());
    }

    #[test]
    fn no_candidate_when_flagship_is_better() {
        let mut p = vec![
            entry(
                SegmentId::Seg01ImprovisedRoom,
                S::Flagship,
                &[1.0, 1.0, 1.0],
            ),
            entry(SegmentId::Seg02aHexmap, S::Regular, &[0.5, 0.5, 0.5]),
        ];
        assert_eq!(flag_flagship_candidate(&mut p), None);
        assert!(p.iter().all(|e| !e.pending_owner));
    }
}
