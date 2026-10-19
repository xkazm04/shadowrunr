//! Operational memory: the in-memory working set (jobs, the open poll, the active foil,
//! recent lines not to repeat) with a time-to-live, and the episode window fitted to a
//! character budget by bisection.

use std::collections::{BTreeMap, VecDeque};

use shadowrunr_types::{Op, TsMs};

use crate::pool::{JobStatus, JobUpdate};
use crate::util::one_line;

/// Entries untouched for this long leave the working set.
pub const WORKING_SET_TTL_MS: i64 = 3_600_000;
const RECENT_LINES: usize = 12;
/// Finished research results the prompt lists under "Earlier results", newest kept. A long
/// investigation dispatches dozens of jobs inside the TTL; the segment's state summary keeps
/// the case file's findings, so older summaries only cost prompt.
pub const EARLIER_RESULTS: usize = 8;

#[derive(Debug, Clone)]
pub struct JobEntry {
    pub job_id: String,
    pub brief: String,
    pub status: String,
    pub summary: Option<String>,
    pub updated_ms: TsMs,
    /// Finished since the last composed prompt (shown as news once).
    pub fresh: bool,
}

#[derive(Debug, Clone)]
pub struct PollInfo {
    pub poll_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub opened_ms: TsMs,
}

#[derive(Debug, Clone)]
pub struct WorkingSet {
    pub jobs: BTreeMap<String, JobEntry>,
    pub active_poll: Option<PollInfo>,
    pub active_foil: Option<String>,
    /// Recent spoken lines, newest last: do not repeat these jokes.
    pub recent_lines: VecDeque<String>,
    ttl_ms: i64,
}

impl Default for WorkingSet {
    fn default() -> Self {
        Self::new(WORKING_SET_TTL_MS)
    }
}

impl WorkingSet {
    pub fn new(ttl_ms: i64) -> Self {
        Self {
            jobs: BTreeMap::new(),
            active_poll: None,
            active_foil: None,
            recent_lines: VecDeque::new(),
            ttl_ms,
        }
    }

    pub fn apply_job(&mut self, u: &JobUpdate, now_ms: TsMs) {
        let e = self
            .jobs
            .entry(u.job_id.clone())
            .or_insert_with(|| JobEntry {
                job_id: u.job_id.clone(),
                brief: u.brief.clone(),
                status: String::new(),
                summary: None,
                updated_ms: now_ms,
                fresh: false,
            });
        e.status = u.status.as_str().to_string();
        if let Some(s) = u.status.summary() {
            e.summary = Some(s.to_string());
        }
        e.updated_ms = now_ms;
        if matches!(u.status, JobStatus::Done(_) | JobStatus::Failed(_)) {
            e.fresh = true;
        }
    }

    /// Track state-changing ops the character emitted.
    pub fn apply_op(&mut self, op: &Op, now_ms: TsMs) {
        match op {
            Op::StartPoll {
                poll_id,
                question,
                options,
                ..
            } => {
                self.active_poll = Some(PollInfo {
                    poll_id: poll_id.clone(),
                    question: question.clone(),
                    options: options.clone(),
                    opened_ms: now_ms,
                })
            }
            Op::ClosePoll { poll_id } => {
                if self.active_poll.as_ref().map(|p| &p.poll_id) == Some(poll_id) {
                    self.active_poll = None;
                }
            }
            Op::SummonFoil { foil } => self.active_foil = Some(foil.clone()),
            Op::DismissFoil => self.active_foil = None,
            _ => {}
        }
    }

    pub fn remember_line(&mut self, line: &str) {
        self.recent_lines.push_back(one_line(line, 140));
        while self.recent_lines.len() > RECENT_LINES {
            self.recent_lines.pop_front();
        }
    }

    /// Drop entries untouched for longer than the TTL. Open jobs stay while running.
    pub fn expire(&mut self, now_ms: TsMs) {
        let ttl = self.ttl_ms;
        self.jobs.retain(|_, j| {
            let open = j.status == "queued" || j.status == "running" || j.status == "progress";
            open || now_ms - j.updated_ms < ttl
        });
        if self
            .active_poll
            .as_ref()
            .is_some_and(|p| now_ms - p.opened_ms >= ttl)
        {
            self.active_poll = None;
        }
    }

    /// Render the working-set block. Marks fresh results as seen.
    pub fn render(&mut self, foil_text: Option<&str>) -> String {
        let mut s = String::new();
        let open: Vec<&JobEntry> = self
            .jobs
            .values()
            .filter(|j| matches!(j.status.as_str(), "queued" | "running" | "progress"))
            .collect();
        if !open.is_empty() {
            s.push_str("Research minions still working:\n");
            for j in open {
                s.push_str(&format!(
                    "- {} ({}): {}\n",
                    j.job_id,
                    j.status,
                    one_line(&j.brief, 120)
                ));
            }
        }
        let fresh: Vec<&JobEntry> = self.jobs.values().filter(|j| j.fresh).collect();
        if !fresh.is_empty() {
            s.push_str("Results just in (announce them if they fit the moment):\n");
            for j in fresh {
                s.push_str(&format!(
                    "- {} {}: {}\n",
                    j.job_id,
                    j.status,
                    one_line(j.summary.as_deref().unwrap_or(""), 600)
                ));
            }
        }
        let mut older: Vec<&JobEntry> = self
            .jobs
            .values()
            .filter(|j| !j.fresh && matches!(j.status.as_str(), "done" | "failed"))
            .collect();
        if !older.is_empty() {
            // Newest first by update time (ties by id), then the cap.
            older.sort_by(|a, b| {
                b.updated_ms
                    .cmp(&a.updated_ms)
                    .then_with(|| a.job_id.cmp(&b.job_id))
            });
            let omitted = older.len().saturating_sub(EARLIER_RESULTS);
            older.truncate(EARLIER_RESULTS);
            older.reverse();
            s.push_str("Earlier results:\n");
            if omitted > 0 {
                s.push_str(&format!("({omitted} older results omitted)\n"));
            }
            for j in older {
                s.push_str(&format!(
                    "- {}: {}\n",
                    j.job_id,
                    one_line(j.summary.as_deref().unwrap_or(""), 200)
                ));
            }
        }
        for j in self.jobs.values_mut() {
            j.fresh = false;
        }
        if let Some(p) = &self.active_poll {
            s.push_str(&format!(
                "Open poll {}: {} [{}]\n",
                p.poll_id,
                one_line(&p.question, 160),
                p.options.join(" | ")
            ));
        }
        if let Some(f) = &self.active_foil {
            s.push_str(&format!("Active foil: {f}\n"));
            if let Some(t) = foil_text {
                s.push_str(t.trim());
                s.push('\n');
            }
        }
        if !self.recent_lines.is_empty() {
            s.push_str("Your recent lines (do not repeat these jokes):\n");
            for l in &self.recent_lines {
                s.push_str(&format!("- {l}\n"));
            }
        }
        s
    }
}

/// One item of the episode window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowItem {
    pub ts_ms: TsMs,
    pub label: String,
    pub text: String,
}

pub const EPISODE_WINDOW: usize = 20;
pub const RECENCY_FLOOR: usize = 6;
pub const EPISODE_BUDGET_CHARS: usize = 24_000;

/// Choose up to `EPISODE_WINDOW` items: at least `RECENCY_FLOOR` of the most recent, the
/// rest from query-relevant older items. Returned oldest first.
pub fn select_window(relevant: Vec<WindowItem>, recent: Vec<WindowItem>) -> Vec<WindowItem> {
    let tail_n = recent
        .len()
        .min(RECENCY_FLOOR.max(EPISODE_WINDOW.saturating_sub(relevant.len())));
    let tail: Vec<WindowItem> = recent[recent.len() - tail_n..].to_vec();
    let mut out: Vec<WindowItem> = Vec::new();
    for r in relevant {
        if out.len() + tail.len() >= EPISODE_WINDOW {
            break;
        }
        if !tail.iter().any(|t| t.label == r.label) && !out.iter().any(|o| o.label == r.label) {
            out.push(r);
        }
    }
    out.sort_by_key(|w| w.ts_ms);
    out.extend(tail);
    out
}

pub fn render_window(items: &[WindowItem], omitted: usize) -> String {
    let mut s = String::new();
    if omitted > 0 {
        s.push_str(&format!("({omitted} older episodes omitted)\n"));
    }
    for w in items {
        s.push_str(&format!("[{}] {}\n", w.label, w.text.trim()));
    }
    s
}

/// The largest number of TRAILING items whose rendered section fits `budget`, found by
/// bisection on the rendered artifact (the header is paid once, so per-item pricing would
/// be wrong). At least 1 for a non-empty list: the current context is never dropped.
pub fn fit_trailing<T>(items: &[T], budget: usize, render: impl Fn(&[T]) -> String) -> usize {
    let n = items.len();
    if n == 0 {
        return 0;
    }
    if render(items).len() <= budget {
        return n;
    }
    let (mut lo, mut hi) = (1usize, n - 1);
    let mut best = 1usize;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        if render(&items[n - mid..]).len() <= budget {
            best = mid;
            lo = mid + 1;
        } else {
            if mid == 1 {
                break;
            }
            hi = mid - 1;
        }
    }
    best
}

/// Fit the window to the budget and render it with the "N older omitted" note.
pub fn fitted_window(items: &[WindowItem], budget: usize) -> String {
    let render = |sl: &[WindowItem]| render_window(sl, items.len() - sl.len());
    let k = fit_trailing(items, budget, render);
    render_window(&items[items.len() - k..], items.len() - k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(i: i64, len: usize) -> WindowItem {
        WindowItem {
            ts_ms: i,
            label: format!("e{i}"),
            text: "x".repeat(len),
        }
    }

    #[test]
    fn window_keeps_recency_floor_and_cap() {
        let relevant: Vec<_> = (0..30).map(|i| item(i, 10)).collect();
        let recent: Vec<_> = (100..110).map(|i| item(i, 10)).collect();
        let w = select_window(relevant, recent);
        assert_eq!(w.len(), EPISODE_WINDOW);
        let tail: Vec<i64> = w[w.len() - RECENCY_FLOOR..]
            .iter()
            .map(|x| x.ts_ms)
            .collect();
        assert_eq!(tail, vec![104, 105, 106, 107, 108, 109]);
        assert!(w.windows(2).all(|p| p[0].ts_ms <= p[1].ts_ms));
    }

    #[test]
    fn bisection_fits_budget_and_announces_omission() {
        let items: Vec<_> = (0..20).map(|i| item(i, 2_000)).collect();
        let s = fitted_window(&items, EPISODE_BUDGET_CHARS);
        assert!(s.len() <= EPISODE_BUDGET_CHARS, "{}", s.len());
        assert!(s.starts_with("(9 older episodes omitted)"), "{}", &s[..40]);
        assert!(s.contains("[e19]"));
        assert!(!s.contains("[e8]"));
        // A single oversize item is still admitted.
        let big = vec![item(1, 30_000)];
        assert!(fitted_window(&big, 1_000).contains("[e1]"));
    }

    #[test]
    fn earlier_results_keep_only_the_newest() {
        let mut ws = WorkingSet::new(10_000_000);
        for k in 0..20u32 {
            let u = JobUpdate {
                job_id: format!("job-{k:02}"),
                origin: "s".into(),
                brief: "b".into(),
                status: JobStatus::Done(format!("result {k}")),
            };
            ws.apply_job(&u, i64::from(k) * 1_000);
        }
        let first = ws.render(None);
        assert!(first.contains("Results just in"), "{first}");
        let s = ws.render(None);
        assert!(s.contains("(12 older results omitted)"), "{s}");
        assert!(s.contains("job-19") && s.contains("job-12"), "{s}");
        assert!(!s.contains("job-11:"), "{s}");
        let at = |id: &str| s.find(id).unwrap();
        assert!(at("job-12") < at("job-19"), "oldest kept first");
    }

    #[test]
    fn ttl_expires_finished_jobs_and_keeps_open_ones() {
        let mut ws = WorkingSet::new(1_000);
        let up = |id: &str, st: JobStatus| JobUpdate {
            job_id: id.into(),
            origin: "s".into(),
            brief: "b".into(),
            status: st,
        };
        ws.apply_job(&up("a", JobStatus::Done("ok".into())), 0);
        ws.apply_job(&up("b", JobStatus::Running), 0);
        ws.expire(5_000);
        assert!(!ws.jobs.contains_key("a"));
        assert!(ws.jobs.contains_key("b"));
    }
}
