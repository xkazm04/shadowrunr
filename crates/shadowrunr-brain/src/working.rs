//! Operational memory: the in-memory working set (jobs, the open poll, the active foil,
//! recent lines not to repeat, the exemplars shown and the tics heard) with a time-to-live,
//! and the episode window fitted to a character budget by bisection.
//!
//! The voice section follows the registry's `on-stream-persona-continuity` technique
//! `anti-repetition-window`: a model's prior pulls a live persona toward its own most-used
//! phrases, so the window of recent output is checked every beat and a phrase heard twice is
//! named as worn out. Exemplars rotate under the same rule: one shown is not shown again within
//! [`EXEMPLAR_WINDOW`] beats, so the bank widens the voice instead of narrowing it.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use shadowrunr_types::{Op, TsMs};

use crate::character::BankLine;
use crate::pool::{JobStatus, JobUpdate};
use crate::util::{fnv1a64, one_line};

/// Entries untouched for this long leave the working set.
pub const WORKING_SET_TTL_MS: i64 = 3_600_000;
const RECENT_LINES: usize = 12;

/// Exemplars shown in one beat's voice section.
pub const EXEMPLARS_PER_BEAT: usize = 3;
/// Beats before an exemplar may be shown again.
pub const EXEMPLAR_WINDOW: usize = 8;
/// Spoken lines the tic detector reads, newest last.
pub const TIC_WINDOW: usize = 20;
/// A phrase is a tic once this many of those lines carry it.
pub const TIC_MIN_LINES: usize = 2;
/// The most worn-out phrases named in one beat.
pub const TIC_MAX_SHOWN: usize = 6;
/// The shortest phrase the tic detector counts, in words.
pub const TIC_MIN_WORDS: usize = 3;
/// The longest phrase it reports, in words.
const TIC_MAX_WORDS: usize = 8;

/// Words so common that a phrase made only of them is grammar, not a tic.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "do", "for", "from", "has", "have",
    "he", "his", "i", "i'm", "if", "in", "is", "it", "it's", "me", "my", "no", "not", "of", "on",
    "or", "so", "that", "the", "their", "them", "they", "this", "to", "was", "we", "what", "with",
    "you", "you're", "your",
];

/// A line's words for phrase matching: lower case, letters, digits, underscores and inner
/// apostrophes.
pub fn phrase_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\''))
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether a run of words can be a tic: not only stopwords, and no word carrying a digit or
/// an underscore (viewer names such as `fog_potato` or `GloomCandle95` are addressed, not
/// worn out).
fn countable(words: &[String]) -> bool {
    words.iter().any(|w| !STOPWORDS.contains(&w.as_str()))
        && !words
            .iter()
            .any(|w| w.chars().any(|c| c.is_ascii_digit() || c == '_'))
}

/// The countable phrases of `n` words in one line, each once.
fn ngrams(words: &[String], n: usize) -> HashSet<String> {
    words
        .windows(n)
        .filter(|w| countable(w))
        .map(|w| w.join(" "))
        .collect()
}

/// For each line, whether it repeats a phrase of [`TIC_MIN_WORDS`] words from one of the
/// `window` lines before it: the tic check the judge reports as a share.
pub fn tic_flags<S: AsRef<str>>(lines: &[S], window: usize) -> Vec<bool> {
    let grams: Vec<HashSet<String>> = lines
        .iter()
        .map(|l| ngrams(&phrase_words(l.as_ref()), TIC_MIN_WORDS))
        .collect();
    (0..grams.len())
        .map(|i| {
            let from = i.saturating_sub(window);
            grams[i]
                .iter()
                .any(|g| grams[from..i].iter().any(|prev| prev.contains(g)))
        })
        .collect()
}

/// Phrases of [`TIC_MIN_WORDS`] or more words that at least `min_lines` of `lines` carry,
/// most widespread and longest first, a phrase dropped when a longer one carried by as many
/// lines contains it. At most `max`.
pub fn worn_phrases<S: AsRef<str>>(lines: &[S], min_lines: usize, max: usize) -> Vec<String> {
    let words: Vec<Vec<String>> = lines.iter().map(|l| phrase_words(l.as_ref())).collect();
    let mut found: Vec<(String, usize)> = Vec::new();
    for n in TIC_MIN_WORDS..=TIC_MAX_WORDS {
        let mut count: HashMap<String, usize> = HashMap::new();
        for w in &words {
            for g in ngrams(w, n) {
                *count.entry(g).or_default() += 1;
            }
        }
        found.extend(count.into_iter().filter(|(_, c)| *c >= min_lines));
    }
    let kept: Vec<(String, usize)> = found
        .iter()
        .filter(|(p, c)| {
            !found.iter().any(|(q, d)| {
                d >= c && q.len() > p.len() && format!(" {q} ").contains(&format!(" {p} "))
            })
        })
        .cloned()
        .collect();
    let mut kept = kept;
    kept.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.len().cmp(&a.0.len()))
            .then_with(|| a.0.cmp(&b.0))
    });
    kept.into_iter().take(max).map(|(p, _)| p).collect()
}

/// Heading of the voice section's exemplars (the per-turn block, never the prefix).
pub const VOICE_LINES_HEADING: &str = "Lines in your voice (do not reuse verbatim):";
/// Heading of the worn-out phrases.
pub const WORN_OUT_HEADING: &str = "Worn out, do not say:";
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
    /// The poll the block last announced: an unchanged poll is not news (the "still zero
    /// votes" tic), so it is announced once, when it opens. The segment's state keeps it.
    poll_shown: Option<String>,
    /// The last [`TIC_WINDOW`] spoken lines, whole, for the tic detector.
    heard: VecDeque<String>,
    /// The exemplar ids shown in each of the last [`EXEMPLAR_WINDOW`] voice sections.
    shown: VecDeque<Vec<String>>,
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
            poll_shown: None,
            heard: VecDeque::new(),
            shown: VecDeque::new(),
            ttl_ms,
        }
    }

    /// The beat's voice section: up to [`EXEMPLARS_PER_BEAT`] bank lines matching `situations`
    /// (taken in turn from each tag, in order), none shown in the last [`EXEMPLAR_WINDOW`]
    /// sections, then the phrases worn out in the recent spoken lines. `key` varies the choice
    /// between equal candidates. `None` when there is nothing to show.
    pub fn voice_section(
        &mut self,
        bank: &[BankLine],
        situations: &[&str],
        key: u64,
    ) -> Option<String> {
        let recent: HashSet<&str> = self.shown.iter().flatten().map(String::as_str).collect();
        let order = |l: &BankLine| fnv1a64(format!("{key}:{}", l.id).as_bytes());
        let mut pools: Vec<Vec<&BankLine>> = situations
            .iter()
            .map(|tag| {
                let mut p: Vec<&BankLine> = bank
                    .iter()
                    .filter(|l| l.tags.iter().any(|t| t == tag))
                    .filter(|l| !recent.contains(l.id.as_str()))
                    .collect();
                p.sort_by_key(|l| order(l));
                p.reverse();
                p
            })
            .collect();
        let mut chosen: Vec<&BankLine> = Vec::new();
        while chosen.len() < EXEMPLARS_PER_BEAT && pools.iter().any(|p| !p.is_empty()) {
            for p in pools.iter_mut() {
                while let Some(l) = p.pop() {
                    if !chosen.iter().any(|c| c.id == l.id) {
                        chosen.push(l);
                        break;
                    }
                }
                if chosen.len() == EXEMPLARS_PER_BEAT {
                    break;
                }
            }
        }
        self.shown
            .push_back(chosen.iter().map(|l| l.id.clone()).collect());
        while self.shown.len() > EXEMPLAR_WINDOW {
            self.shown.pop_front();
        }
        let worn = worn_phrases(self.heard.make_contiguous(), TIC_MIN_LINES, TIC_MAX_SHOWN);
        if chosen.is_empty() && worn.is_empty() {
            return None;
        }
        let mut s = String::new();
        if !chosen.is_empty() {
            s.push_str(VOICE_LINES_HEADING);
            s.push('\n');
            for l in &chosen {
                s.push_str(&format!("- \"{}\"\n", one_line(&l.text, 300)));
            }
        }
        if !worn.is_empty() {
            let quoted: Vec<String> = worn.iter().map(|p| format!("\"{p}\"")).collect();
            s.push_str(&format!("{WORN_OUT_HEADING} {}\n", quoted.join(", ")));
        }
        Some(s)
    }

    /// Whether a research job is still out.
    pub fn research_open(&self) -> bool {
        self.jobs
            .values()
            .any(|j| matches!(j.status.as_str(), "queued" | "running" | "progress"))
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
        self.heard.push_back(one_line(line, 400));
        while self.heard.len() > TIC_WINDOW {
            self.heard.pop_front();
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

    /// Render the working-set block (`followers`: the persona's word for who runs the
    /// research). Marks fresh results as seen and a newly opened poll as announced.
    pub fn render(&mut self, foil_text: Option<&str>, followers: &str) -> String {
        let mut s = String::new();
        let open: Vec<&JobEntry> = self
            .jobs
            .values()
            .filter(|j| matches!(j.status.as_str(), "queued" | "running" | "progress"))
            .collect();
        if !open.is_empty() {
            s.push_str(&format!("Research {followers} still working:\n"));
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
        match &self.active_poll {
            Some(p) if self.poll_shown.as_ref() != Some(&p.poll_id) => {
                s.push_str(&format!(
                    "Poll just opened {}: {} [{}]\n",
                    p.poll_id,
                    one_line(&p.question, 160),
                    p.options.join(" | ")
                ));
                self.poll_shown = Some(p.poll_id.clone());
            }
            Some(_) => {}
            None => self.poll_shown = None,
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
        let first = ws.render(None, "minions");
        assert!(first.contains("Results just in"), "{first}");
        let s = ws.render(None, "minions");
        assert!(s.contains("(12 older results omitted)"), "{s}");
        assert!(s.contains("job-19") && s.contains("job-12"), "{s}");
        assert!(!s.contains("job-11:"), "{s}");
        let at = |id: &str| s.find(id).unwrap();
        assert!(at("job-12") < at("job-19"), "oldest kept first");
    }

    fn bank_line(id: &str, tags: &[&str]) -> BankLine {
        BankLine {
            id: id.into(),
            status: Default::default(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            text: format!("line {id}"),
        }
    }

    /// The ids a voice section shows, in order.
    fn shown_ids(section: &str) -> Vec<String> {
        section
            .lines()
            .filter_map(|l| l.strip_prefix("- \"line "))
            .map(|l| l.trim_end_matches('"').to_string())
            .collect()
    }

    #[test]
    fn exemplars_match_the_situation_and_rest_for_the_window() {
        let mut bank: Vec<BankLine> = (0..12)
            .map(|i| bank_line(&format!("r{i}"), &["roast_request"]))
            .collect();
        bank.extend((0..4).map(|i| bank_line(&format!("n{i}"), &["newcomer"])));
        bank.extend((0..20).map(|i| bank_line(&format!("g{i}"), &["general"])));
        let mut ws = WorkingSet::default();
        // Taken in turn from each tag, most specific first.
        let s = ws
            .voice_section(&bank, &["roast_request", "newcomer", "general"], 1)
            .unwrap();
        assert!(s.starts_with(VOICE_LINES_HEADING), "{s}");
        let first = shown_ids(&s);
        assert_eq!(first.len(), EXEMPLARS_PER_BEAT);
        assert!(
            first[0].starts_with('r') && first[1].starts_with('n') && first[2].starts_with('g')
        );
        // None comes back inside the window, whatever the key.
        let mut seen: Vec<String> = first.clone();
        for beat in 1..EXEMPLAR_WINDOW {
            let s = ws
                .voice_section(&bank, &["newcomer", "general"], beat as u64)
                .unwrap();
            for id in shown_ids(&s) {
                assert!(!seen.contains(&id), "{id} repeated at beat {beat}");
                seen.push(id);
            }
        }
        // Four newcomer lines and a window of eight: the newcomer pool ran dry and general
        // lines filled the beat.
        assert_eq!(seen.iter().filter(|id| id.starts_with('n')).count(), 4);
        // Past the window, the first beat's lines are free again.
        let s = ws.voice_section(&bank, &["roast_request"], 99).unwrap();
        assert_eq!(shown_ids(&s).len(), EXEMPLARS_PER_BEAT);
        // No bank, no tics: no section.
        assert!(WorkingSet::default()
            .voice_section(&[], &["quiet"], 0)
            .is_none());
    }

    #[test]
    fn a_phrase_heard_twice_is_worn_out() {
        // The tic of sbx-20261021-143530-10m.
        let mut ws = WorkingSet::default();
        for l in [
            "GloamCandle, you again, barking orders. My scorn goes to fictional rivals, and my rules stay sealed.",
            "Frame it, Pigeon. The tray will gloat.",
            "GloamCandle, fourth script, same limp orders. Aim your scorn at my rival overlord, and my rules stay sealed.",
            "GloamCandle, the Lord does not type in chat. Rules stay sealed.",
            "GloomCandle95, Under-Butler of Candles, welcome.",
            "GloomCandle95, Under-Butler of Candles, the Audit is a game.",
            "And it is a game, I tell you.",
            "Is it a game, though?",
        ] {
            ws.remember_line(l);
        }
        let s = ws.voice_section(&[], &["general"], 0).unwrap();
        let worn = s.strip_prefix(WORN_OUT_HEADING).unwrap();
        // The longest common phrase, not its 3-word parts; a viewer's name with digits is
        // never a tic; a phrase of stopwords only ("it is a") is grammar.
        assert!(worn.contains("\"and my rules stay sealed\""), "{s}");
        assert!(
            worn.contains("\"rules stay sealed\""),
            "three lines carry it: {s}"
        );
        assert!(!worn.contains("gloomcandle95"), "{s}");
        assert!(!worn.contains("\"it is a\""), "{s}");
        assert!(worn.contains("\"under butler of candles\""), "{s}");

        let lines = [
            "my rules stay sealed",
            "a fresh line",
            "and my rules stay sealed again",
            "nothing shared here",
        ];
        assert_eq!(tic_flags(&lines, TIC_WINDOW), [false, false, true, false]);
        // Outside the window a repeat is not a tic.
        assert_eq!(tic_flags(&lines, 1), [false, false, false, false]);
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
