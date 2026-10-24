//! Operational memory: the in-memory working set (jobs, the open poll, the active foil,
//! recent lines not to repeat, the tics heard and the viewer names seen) with a time-to-live,
//! and the episode window fitted to a character budget by bisection.
//!
//! The voice section follows the registry's `on-stream-persona-continuity` technique
//! `anti-repetition-window`: a model's prior pulls a live persona toward its own most-used
//! phrases, so the window of recent output is checked every beat and a phrase heard twice is
//! named as worn out, and an opener repeated (a viewer's handle first, or the same first two
//! words) is named too. It carries no example line: the package's effects (`effects.toml`) say
//! what a reply should achieve instead. Exemplars shown there aired verbatim in 140 of 260 sim
//! runs; an effect gives the model nothing to copy.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use shadowrunr_types::{Op, TsMs};

use crate::character::Effect;
use crate::pool::{JobStatus, JobUpdate};
use crate::util::one_line;

/// Entries untouched for this long leave the working set.
pub const WORKING_SET_TTL_MS: i64 = 3_600_000;
const RECENT_LINES: usize = 12;

/// Effects shown in one beat's voice section.
pub const EFFECTS_PER_BEAT: usize = 3;
/// Host lines the opener guard reads, newest last.
pub const OPENER_WINDOW: usize = 4;
/// The opener guard speaks once this many of them open the same way.
pub const OPENER_MIN_LINES: usize = 2;
/// Viewer names the working set remembers for the opener guard.
const VIEWER_NAMES: usize = 400;
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

/// Heading of the voice section's effects (the per-turn block, never the prefix).
pub const EFFECTS_HEADING: &str = "What lands here:";
/// Heading of the worn-out phrases.
pub const WORN_OUT_HEADING: &str = "Worn out, do not say:";

/// The opener guard's line, `{opener}` filled ([`opener_repeat`]).
pub const OPENER_GUARD: &str =
    "Your last lines opened the same way (\"{opener}\"): open differently this time.";

/// The viewer name a line opens with: `names` (any case) at its very start, followed by a
/// non-name character or the end.
fn opening_name<'a>(line: &str, names: &'a [String]) -> Option<&'a str> {
    let l = line.trim_start().to_lowercase();
    names
        .iter()
        .filter(|n| n.chars().count() >= 3)
        .filter(|n| {
            let n = n.to_lowercase();
            l.starts_with(&n)
                && !l[n.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .max_by_key(|n| n.len())
        .map(String::as_str)
}

/// A line opening with a handle and its rank, "<name> (<Rank>)," or "<name> the <Rank>,": an
/// opener by its shape alone, whether or not the name is known (22 of 65 host lines of
/// sbx-20261024-205313-10m opened "WraithKettle (Thrall), ..."). Returns the handle.
fn opening_titled_handle(line: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"^\s*([\w'’]{3,})\s*(?:\(\s*[^()]{2,30}\)|the\s+[A-Z][\w-]*(?:\s+[A-Z][\w-]*)?)\s*[,:!]",
        )
        .expect("titled-handle pattern compiles")
    })
    .captures(line)
    .map(|c| c[1].to_string())
}

/// The opener the last [`OPENER_WINDOW`] host lines repeat, when [`OPENER_MIN_LINES`] or more of
/// them open with a viewer's name (any of `names`), or share their first two words: the most
/// recent such line's name, or the shared two words. `None` when they vary.
pub fn opener_repeat<S: AsRef<str>>(lines: &[S], names: &[String]) -> Option<String> {
    let tail = &lines[lines.len().saturating_sub(OPENER_WINDOW)..];
    let by_name: Vec<String> = tail
        .iter()
        .filter_map(|l| {
            opening_titled_handle(l.as_ref())
                .or_else(|| opening_name(l.as_ref(), names).map(str::to_string))
        })
        .collect();
    if by_name.len() >= OPENER_MIN_LINES {
        return by_name.last().map(|n| n.to_string());
    }
    let firsts: Vec<String> = tail
        .iter()
        .map(|l| phrase_words(l.as_ref()))
        .filter(|w| w.len() >= 2)
        .map(|w| format!("{} {}", w[0], w[1]))
        .collect();
    firsts
        .iter()
        .rev()
        .find(|f| firsts.iter().filter(|g| g == f).count() >= OPENER_MIN_LINES)
        .cloned()
}
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
    /// Viewer names seen in chat lately, newest last, for the opener guard.
    viewer_names: VecDeque<String>,
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
            viewer_names: VecDeque::new(),
            ttl_ms,
        }
    }

    /// Remember the names of viewers seen in chat (the opener guard's handles), newest last.
    pub fn note_viewers<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        for n in names {
            let n = n.trim();
            if n.chars().count() < 3 {
                continue;
            }
            if let Some(i) = self.viewer_names.iter().position(|x| x == n) {
                self.viewer_names.remove(i);
            }
            self.viewer_names.push_back(n.to_string());
        }
        while self.viewer_names.len() > VIEWER_NAMES {
            self.viewer_names.pop_front();
        }
    }

    /// The beat's voice section: the effects of up to [`EFFECTS_PER_BEAT`] of `situations` (in
    /// order, each tag's one effect), then the phrases worn out in the recent spoken lines, then
    /// the opener guard when the last host lines opened the same way ([`opener_repeat`]; a
    /// viewer's name from `names` or the working set's own, or the same first two words).
    /// `None` when there is nothing to show.
    pub fn voice_section(
        &mut self,
        effects: &[Effect],
        situations: &[&str],
        names: &[String],
    ) -> Option<String> {
        let chosen: Vec<&Effect> = situations
            .iter()
            .filter_map(|tag| effects.iter().find(|e| e.tag == *tag))
            .take(EFFECTS_PER_BEAT)
            .collect();
        let heard: Vec<String> = self.heard.iter().cloned().collect();
        let worn = worn_phrases(&heard, TIC_MIN_LINES, TIC_MAX_SHOWN);
        let mut all_names: Vec<String> = names.to_vec();
        all_names.extend(self.viewer_names.iter().cloned());
        let opener = opener_repeat(&heard, &all_names);
        if chosen.is_empty() && worn.is_empty() && opener.is_none() {
            return None;
        }
        let mut s = String::new();
        if !chosen.is_empty() {
            s.push_str(EFFECTS_HEADING);
            s.push('\n');
            for e in &chosen {
                s.push_str(&format!("- {}\n", one_line(&e.text, 300)));
            }
        }
        if !worn.is_empty() {
            let quoted: Vec<String> = worn.iter().map(|p| format!("\"{p}\"")).collect();
            s.push_str(&format!("{WORN_OUT_HEADING} {}\n", quoted.join(", ")));
        }
        if let Some(o) = opener {
            s.push_str(&OPENER_GUARD.replace("{opener}", &one_line(&o, 60)));
            s.push('\n');
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

    fn effect(tag: &str) -> Effect {
        Effect {
            tag: tag.into(),
            text: format!("the {tag} effect"),
        }
    }

    #[test]
    fn effects_follow_the_situation_order_at_most_three_and_quote_no_line() {
        let effects: Vec<Effect> = ["general", "newcomer", "gift", "bait", "care"]
            .iter()
            .map(|t| effect(t))
            .collect();
        let mut ws = WorkingSet::default();
        let s = ws
            .voice_section(&effects, &["bait", "gift", "newcomer", "general"], &[])
            .unwrap();
        assert_eq!(
            s,
            format!(
                "{EFFECTS_HEADING}\n- the bait effect\n- the gift effect\n- the newcomer effect\n"
            )
        );
        // A tag without an effect is skipped; the same effects every beat (no rotation).
        let s = ws.voice_section(&effects, &["quiet", "care"], &[]).unwrap();
        assert_eq!(s, format!("{EFFECTS_HEADING}\n- the care effect\n"));
        // No effects, no tics, no repeated opener: no section.
        assert!(WorkingSet::default()
            .voice_section(&[], &["quiet"], &[])
            .is_none());
    }

    #[test]
    fn a_repeated_opener_is_named_handle_or_first_two_words() {
        // The shape of the 3-hour run: every reply opened "rune_turnip, Dread Council,".
        let names = vec!["rune_turnip".to_string(), "Grimwald".to_string()];
        let lines = [
            "A fine night for it.",
            "rune_turnip, Dread Council, the moat is yours.",
            "Grimwald! You again.",
            "The ledger waits.",
        ];
        assert_eq!(opener_repeat(&lines, &names).as_deref(), Some("Grimwald"));
        // Two handles out of the last four, whatever the handles.
        assert_eq!(
            opener_repeat(&lines[..3], &names).as_deref(),
            Some("Grimwald")
        );
        // A name inside a longer word is no opener ("Grimwaldo"), and one handle is not a habit.
        let one = [
            "Grimwaldo rises.",
            "rune_turnip, hello.",
            "Quiet now.",
            "Then the vote.",
        ];
        assert_eq!(opener_repeat(&one, &names), None);
        // The same first two words, without any names.
        let same = [
            "My lord, the ledger.",
            "Fine.",
            "My lord, the moat.",
            "Next.",
        ];
        assert_eq!(opener_repeat(&same, &[]).as_deref(), Some("my lord"));
        // Only the last four lines count.
        let old = ["My lord, a.", "My lord, b.", "c d", "e f", "g h", "i j"];
        assert_eq!(opener_repeat(&old, &[]), None);

        // In the voice section, from the lines he spoke and the names seen in chat.
        let mut ws = WorkingSet::default();
        ws.note_viewers(["rune_turnip", "x"]);
        for l in [
            "rune_turnip, Dread Council, the throne is yours.",
            "A pause.",
            "rune_turnip, Dread Council, again?",
        ] {
            ws.remember_line(l);
        }
        let s = ws.voice_section(&[], &["general"], &[]).unwrap();
        assert!(
            s.ends_with(
                "Your last lines opened the same way (\"rune_turnip\"): open differently this \
                 time.\n"
            ),
            "{s}"
        );
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
        let s = ws.voice_section(&[], &["general"], &[]).unwrap();
        let mut section = s.lines();
        let worn = section
            .next()
            .unwrap()
            .strip_prefix(WORN_OUT_HEADING)
            .unwrap();
        // The last four lines open "GloomCandle95, Under-Butler" twice: the opener guard.
        assert_eq!(
            section.next(),
            Some(
                "Your last lines opened the same way (\"gloomcandle95 under\"): open differently \
                 this time."
            )
        );
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

    #[test]
    fn a_handle_with_its_rank_is_an_opener_by_its_shape() {
        // sbx-20261024-205313-10m: 22 of 65 host lines opened "<name> (<Rank>), ...".
        let lines = [
            "WraithKettle (Thrall), yes, I am an AI character.",
            "The ledger turns a page.",
            "MothWalrus (Warlock), the vote is yours.",
        ];
        assert_eq!(opener_repeat(&lines, &[]).as_deref(), Some("MothWalrus"));
        let spoken = [
            "WraithKettle the Thrall, yes, I am an AI character.",
            "MothWalrus the Warlock, the vote is yours.",
        ];
        assert_eq!(opener_repeat(&spoken, &[]).as_deref(), Some("MothWalrus"));
        // Neighbour: one such line is no habit, and a bracket later in a line is no opener.
        let fine = [
            "WraithKettle (Thrall), yes.",
            "The card (HEX-11) stays fogged.",
            "A quiet beat.",
        ];
        assert_eq!(opener_repeat(&fine, &[]), None);
    }
}
