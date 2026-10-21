//! `voice replay --arms "workers=2,3 flight=1,3" --repeat 3 [--baseline workers=3,flight=3]`:
//! every arm of the grid, and the baseline, replayed `repeat` times over the same transcript,
//! interleaved (each repeat runs every arm once before the next starts, so a change in machine
//! load lands on all of them). Each figure is printed as its median with its range and n, and
//! each arm's lines are diffed against the baseline's: the lines it saves, the lines it loses,
//! and the ones that fail for another reason. A line's outcome over the repeats is its majority.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use serde::Serialize;

use super::{Outcome, Report};
use crate::voice::VoiceConfig;

/// A line's text in the diff is cut to this many characters.
pub const TEXT_CHARS: usize = 60;

/// One config: what it overrides of `[voice]` (with the command's flags applied).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Arm {
    pub workers: Option<usize>,
    pub lines_in_flight: Option<usize>,
    pub threads: Option<u32>,
}

impl Arm {
    pub fn apply(&self, voice: &VoiceConfig) -> VoiceConfig {
        let mut v = voice.clone();
        v.render_workers = self.workers.unwrap_or(v.render_workers);
        v.lines_in_flight = self.lines_in_flight.unwrap_or(v.lines_in_flight);
        v.render_threads = self.threads.or(v.render_threads);
        v
    }
}

/// The figures that tell two configs apart: workers, lines in flight, threads.
fn key(v: &VoiceConfig) -> (usize, usize, Option<u32>) {
    (
        v.render_workers.max(1),
        v.lines_in_flight.max(1),
        v.render_threads,
    )
}

fn label(v: &VoiceConfig) -> String {
    let (w, f, t) = key(v);
    let t = t.map_or_else(|| "auto".to_string(), |t| t.to_string());
    format!("workers {w} flight {f} threads {t}")
}

/// `key=v1,v2 key=v3`: keys `workers`, `flight` (or `lines_in_flight`), `threads`; values are
/// whole numbers of at least 1. Commas and spaces both separate, so `workers=3,flight=3` reads
/// too. Each key at most once.
fn keys(spec: &str) -> Result<Vec<(&'static str, Vec<u32>)>, String> {
    let mut out: Vec<(&'static str, Vec<u32>)> = Vec::new();
    for tok in spec
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
    {
        let value = match tok.split_once('=') {
            Some((k, v)) => {
                let k = match k.trim() {
                    "workers" => "workers",
                    "flight" | "lines_in_flight" | "lines-in-flight" => "flight",
                    "threads" => "threads",
                    other => {
                        return Err(format!(
                            "unknown arm key `{other}` (workers, flight, threads)"
                        ))
                    }
                };
                if out.iter().any(|(seen, _)| *seen == k) {
                    return Err(format!("`{k}` is given twice"));
                }
                out.push((k, Vec::new()));
                v
            }
            None => tok,
        };
        let Some((k, values)) = out.last_mut() else {
            return Err(format!("`{tok}` has no key (e.g. workers={tok})"));
        };
        if value.is_empty() {
            continue;
        }
        match value.parse::<u32>() {
            Ok(n) if n >= 1 => values.push(n),
            Ok(_) => return Err(format!("{k} must be at least 1")),
            Err(_) => return Err(format!("{k}: `{value}` is not a whole number")),
        }
    }
    if out.is_empty() {
        return Err("an arm needs at least one of workers=, flight=, threads=".into());
    }
    if let Some((k, _)) = out.iter().find(|(_, v)| v.is_empty()) {
        return Err(format!("{k} has no value"));
    }
    Ok(out)
}

/// The grid an `--arms` spec names, workers outermost, then flight, then threads.
pub fn parse_arms(spec: &str) -> Result<Vec<Arm>, String> {
    let keys = keys(spec)?;
    let of = |name: &str| -> Vec<Option<u32>> {
        keys.iter().find(|(k, _)| *k == name).map_or_else(
            || vec![None],
            |(_, v)| v.iter().copied().map(Some).collect(),
        )
    };
    let mut arms = Vec::new();
    for w in of("workers") {
        for f in of("flight") {
            for t in of("threads") {
                let arm = Arm {
                    workers: w.map(|n| n as usize),
                    lines_in_flight: f.map(|n| n as usize),
                    threads: t,
                };
                if !arms.contains(&arm) {
                    arms.push(arm);
                }
            }
        }
    }
    Ok(arms)
}

/// A `--baseline` spec: one value per key.
pub fn parse_baseline(spec: &str) -> Result<Arm, String> {
    match parse_arms(spec)?.as_slice() {
        [one] => Ok(*one),
        _ => Err("--baseline names one config: one value per key".into()),
    }
}

/// What a compare replay runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub baseline: Arm,
    pub arms: Vec<Arm>,
    pub repeat: usize,
}

/// The plan the flags name, or `None` for the plain one-row replay (none of them given).
pub fn plan(
    arms: Option<&str>,
    repeat: Option<usize>,
    baseline: Option<&str>,
) -> Result<Option<Plan>, String> {
    if arms.is_none() && repeat.is_none() && baseline.is_none() {
        return Ok(None);
    }
    if repeat == Some(0) {
        return Err("--repeat must be at least 1".into());
    }
    let arms = match arms {
        Some(spec) => parse_arms(spec).map_err(|e| format!("--arms: {e}"))?,
        None => Vec::new(),
    };
    let baseline = match baseline {
        Some(spec) => parse_baseline(spec).map_err(|e| format!("--baseline: {e}"))?,
        None => Arm::default(),
    };
    Ok(Some(Plan {
        baseline,
        arms,
        repeat: repeat.unwrap_or(1),
    }))
}

/// One config's replays.
#[derive(Debug)]
pub struct ArmRun {
    pub voice: VoiceConfig,
    pub reports: Vec<Report>,
}

/// Every distinct config's replays; `baseline` and `arms` index into `runs` (an arm that is the
/// baseline's config shares its replays).
#[derive(Debug)]
pub struct Grid {
    pub runs: Vec<ArmRun>,
    pub baseline: usize,
    pub arms: Vec<usize>,
}

impl Grid {
    fn base(&self) -> &ArmRun {
        &self.runs[self.baseline]
    }
}

/// Run `plan` over `voice` with `replay` (one replay of one config), interleaved by repeat.
pub async fn run_plan<F, Fut>(
    voice: &VoiceConfig,
    plan: &Plan,
    mut replay: F,
    progress: bool,
) -> Result<Grid, String>
where
    F: FnMut(VoiceConfig) -> Fut,
    Fut: Future<Output = Result<Report, String>>,
{
    let mut runs: Vec<ArmRun> = Vec::new();
    let index = |v: VoiceConfig, runs: &mut Vec<ArmRun>| -> usize {
        match runs.iter().position(|r| key(&r.voice) == key(&v)) {
            Some(i) => i,
            None => {
                runs.push(ArmRun {
                    voice: v,
                    reports: Vec::new(),
                });
                runs.len() - 1
            }
        }
    };
    let baseline = index(plan.baseline.apply(voice), &mut runs);
    let arms: Vec<usize> = plan
        .arms
        .iter()
        .map(|a| index(a.apply(voice), &mut runs))
        .collect();
    for rep in 0..plan.repeat {
        for run in &mut runs {
            if progress {
                eprintln!(
                    "replay: {}, repeat {}/{}",
                    label(&run.voice),
                    rep + 1,
                    plan.repeat
                );
            }
            run.reports.push(replay(run.voice.clone()).await?);
        }
    }
    Ok(Grid {
        runs,
        baseline,
        arms,
    })
}

/// A figure over repeats: its median, range and n.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Spread {
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub n: usize,
}

impl Spread {
    pub fn of(values: impl IntoIterator<Item = f64>) -> Self {
        let mut v: Vec<f64> = values.into_iter().collect();
        if v.is_empty() {
            return Self::default();
        }
        v.sort_by(f64::total_cmp);
        let n = v.len();
        let median = if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        };
        Self {
            median,
            min: v[0],
            max: v[n - 1],
            n,
        }
    }

    /// `1.9 s (1.6-5.3, n=3)` from milliseconds; `-` with no value.
    pub fn seconds_cell(&self) -> String {
        if self.n == 0 {
            return "-".into();
        }
        format!(
            "{:.1} s ({:.1}-{:.1}, n={})",
            self.median / 1000.0,
            self.min / 1000.0,
            self.max / 1000.0,
            self.n
        )
    }

    /// `22 (21-24, n=3)` for a count.
    pub fn count_cell(&self) -> String {
        if self.n == 0 {
            return "-".into();
        }
        let whole = |x: f64| {
            if x.fract() == 0.0 {
                format!("{x:.0}")
            } else {
                format!("{x:.1}")
            }
        };
        format!(
            "{} ({}-{}, n={})",
            whole(self.median),
            whole(self.min),
            whole(self.max),
            self.n
        )
    }

    /// `141.0 (139.2-142.5, n=3)` for seconds of audio.
    pub fn decimal_cell(&self) -> String {
        if self.n == 0 {
            return "-".into();
        }
        format!(
            "{:.1} ({:.1}-{:.1}, n={})",
            self.median, self.min, self.max, self.n
        )
    }
}

/// One config's figures over its repeats. Latencies count only the repeats where a line spoke.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Summary {
    pub repeats: usize,
    pub lines: usize,
    pub spoken: Spread,
    pub partial: Spread,
    pub late: Spread,
    pub queue_full: Spread,
    /// Failed and unresolved lines, as the plain row's `error` column counts them.
    pub error: Spread,
    pub first_audio_p50_ms: Spread,
    pub first_audio_p90_ms: Spread,
    pub stage_dropped: Spread,
    pub heard_s: Spread,
    pub say_to_sound_p50_ms: Spread,
}

pub fn aggregate(reports: &[Report]) -> Summary {
    let count = |f: fn(&Report) -> usize| Spread::of(reports.iter().map(|r| f(r) as f64));
    let ms = |f: fn(&Report) -> (usize, u64)| {
        Spread::of(reports.iter().filter_map(|r| {
            let (n, v) = f(r);
            (n > 0).then_some(v as f64)
        }))
    };
    Summary {
        repeats: reports.len(),
        lines: reports.iter().map(|r| r.lines).max().unwrap_or(0),
        spoken: count(|r| r.spoken),
        partial: count(|r| r.partial),
        late: count(|r| r.late),
        queue_full: count(|r| r.queue_full),
        error: count(|r| r.error + r.unresolved),
        first_audio_p50_ms: ms(|r| (r.first_audio_ms.n, r.first_audio_ms.p50)),
        first_audio_p90_ms: ms(|r| (r.first_audio_ms.n, r.first_audio_ms.p90)),
        stage_dropped: count(|r| r.stage_dropped),
        heard_s: Spread::of(reports.iter().map(|r| r.heard_s)),
        say_to_sound_p50_ms: ms(|r| (r.say_to_sound_ms.n, r.say_to_sound_ms.p50)),
    }
}

/// Each line's outcome across `reports`: the most frequent, ties to the one seen first.
pub fn majority(reports: &[Report]) -> BTreeMap<u64, Outcome> {
    let mut tally: BTreeMap<u64, Vec<(Outcome, usize)>> = BTreeMap::new();
    for r in reports {
        for (seq, o) in &r.outcomes {
            let t = tally.entry(*seq).or_default();
            match t.iter_mut().find(|(seen, _)| seen == o) {
                Some((_, n)) => *n += 1,
                None => t.push((*o, 1)),
            }
        }
    }
    tally
        .into_iter()
        .map(|(seq, t)| {
            let best = t.iter().map(|(_, n)| *n).max().unwrap_or(0);
            let o = t
                .iter()
                .find(|(_, n)| *n == best)
                .map_or(Outcome::Unresolved, |(o, _)| *o);
            (seq, o)
        })
        .collect()
}

/// `text` cut to `n` characters, ending `...` when it was longer.
pub fn cut(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        return text.to_string();
    }
    let mut s: String = text.chars().take(n.saturating_sub(3)).collect();
    s.push_str("...");
    s
}

/// One line whose outcome differs from the baseline's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineChange {
    pub seq: u64,
    pub from: Outcome,
    pub to: Outcome,
    pub text: String,
}

/// An arm's lines against the baseline's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// Not spoken in the baseline, spoken here.
    pub saved: Vec<LineChange>,
    /// Spoken in the baseline, not here.
    pub lost: Vec<LineChange>,
    /// Not spoken in either, for different reasons.
    pub changed: Vec<LineChange>,
}

pub fn compare(
    baseline: &BTreeMap<u64, Outcome>,
    arm: &BTreeMap<u64, Outcome>,
    texts: &BTreeMap<u64, String>,
) -> Diff {
    let mut d = Diff::default();
    let seqs: BTreeSet<u64> = baseline.keys().chain(arm.keys()).copied().collect();
    for seq in seqs {
        let from = baseline.get(&seq).copied().unwrap_or(Outcome::Unresolved);
        let to = arm.get(&seq).copied().unwrap_or(Outcome::Unresolved);
        if from == to {
            continue;
        }
        let change = LineChange {
            seq,
            from,
            to,
            text: cut(texts.get(&seq).map_or("", String::as_str), TEXT_CHARS),
        };
        if to == Outcome::Spoken {
            d.saved.push(change);
        } else if from == Outcome::Spoken {
            d.lost.push(change);
        } else {
            d.changed.push(change);
        }
    }
    d
}

fn texts(reports: &[Report]) -> BTreeMap<u64, String> {
    let mut t = BTreeMap::new();
    for r in reports {
        for (seq, text) in &r.texts {
            t.entry(*seq).or_insert_with(|| text.clone());
        }
    }
    t
}

#[derive(Debug, Serialize)]
struct ArmJson {
    workers: usize,
    lines_in_flight: usize,
    threads: Option<u32>,
    #[serde(flatten)]
    summary: Summary,
    outcomes: BTreeMap<u64, Outcome>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    diff: Option<Diff>,
}

#[derive(Debug, Serialize)]
struct GridJson {
    baseline: ArmJson,
    arms: Vec<ArmJson>,
}

fn arm_json(run: &ArmRun, base: Option<&BTreeMap<u64, Outcome>>) -> ArmJson {
    let (workers, lines_in_flight, threads) = key(&run.voice);
    let outcomes = majority(&run.reports);
    let diff = base.map(|b| compare(b, &outcomes, &texts(&run.reports)));
    ArmJson {
        workers,
        lines_in_flight,
        threads,
        summary: aggregate(&run.reports),
        outcomes,
        diff,
    }
}

/// `{"baseline": {...}, "arms": [{..., "outcomes": {"1": "spoken"}, "saved": [], ...}]}`.
pub fn to_json(grid: &Grid) -> serde_json::Value {
    let base = majority(&grid.base().reports);
    let g = GridJson {
        baseline: arm_json(grid.base(), None),
        arms: grid
            .arms
            .iter()
            .map(|i| arm_json(&grid.runs[*i], Some(&base)))
            .collect(),
    };
    serde_json::to_value(g).unwrap_or_default()
}

/// The compare table and each arm's line diff, as printed.
pub fn table(grid: &Grid) -> String {
    let base = grid.base();
    let s = aggregate(&base.reports);
    let mut out = format!(
        "replay: {} lines, {} repeat(s) per config, interleaved; each figure is the median \
         (min-max, n)\n",
        s.lines, s.repeats
    );
    let head = [
        "",
        "spoken",
        "late",
        "qfull",
        "error",
        "first_p50",
        "first_p90",
        "sound_p50",
        "heard_s",
    ];
    let mut rows: Vec<Vec<String>> = vec![head.iter().map(|h| (*h).to_string()).collect()];
    let mut order = vec![grid.baseline];
    order.extend(grid.arms.iter().filter(|i| **i != grid.baseline));
    for i in &order {
        let run = &grid.runs[*i];
        let s = aggregate(&run.reports);
        let name = if *i == grid.baseline {
            format!("baseline: {}", label(&run.voice))
        } else {
            label(&run.voice)
        };
        rows.push(vec![
            name,
            s.spoken.count_cell(),
            s.late.count_cell(),
            s.queue_full.count_cell(),
            s.error.count_cell(),
            s.first_audio_p50_ms.seconds_cell(),
            s.first_audio_p90_ms.seconds_cell(),
            s.say_to_sound_p50_ms.seconds_cell(),
            s.heard_s.decimal_cell(),
        ]);
    }
    let widths: Vec<usize> = (0..head.len())
        .map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0))
        .collect();
    for r in &rows {
        let cells: Vec<String> = r
            .iter()
            .enumerate()
            .map(|(c, cell)| {
                if c == 0 {
                    format!("{cell:<w$}", w = widths[c])
                } else {
                    format!("{cell:>w$}", w = widths[c])
                }
            })
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    let base_outcomes = majority(&base.reports);
    for i in order.iter().skip(1) {
        let run = &grid.runs[*i];
        let d = compare(
            &base_outcomes,
            &majority(&run.reports),
            &texts(&run.reports),
        );
        out.push_str(&format!(
            "\n{} vs baseline: saved {}, lost {}, changed {}\n",
            label(&run.voice),
            d.saved.len(),
            d.lost.len(),
            d.changed.len()
        ));
        for (what, list) in [
            ("saved", &d.saved),
            ("lost", &d.lost),
            ("changed", &d.changed),
        ] {
            for c in list {
                out.push_str(&format!(
                    "  {what:<8} #{:<4} {:>10} -> {:<10}  \"{}\"\n",
                    c.seq,
                    c.from.as_str(),
                    c.to.as_str(),
                    c.text
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::Percentiles;
    use super::*;

    fn outcomes(v: &[(u64, Outcome)]) -> BTreeMap<u64, Outcome> {
        v.iter().copied().collect()
    }

    #[test]
    fn an_arms_spec_is_a_grid_and_zero_is_refused() {
        let arms = parse_arms("workers=1,3 flight=2").unwrap();
        assert_eq!(
            arms,
            [
                Arm {
                    workers: Some(1),
                    lines_in_flight: Some(2),
                    threads: None
                },
                Arm {
                    workers: Some(3),
                    lines_in_flight: Some(2),
                    threads: None
                },
            ]
        );
        assert_eq!(
            parse_arms("flight=1,3 workers=2,3 threads=2")
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            parse_baseline("workers=3,flight=3").unwrap(),
            Arm {
                workers: Some(3),
                lines_in_flight: Some(3),
                threads: None
            }
        );
        assert!(parse_arms("workers=0").unwrap_err().contains("at least 1"));
        assert!(parse_arms("speed=2").is_err());
        assert!(parse_arms("workers=two").is_err());
        assert!(parse_arms("workers=1 workers=2").is_err());
        assert!(parse_arms("3").is_err());
        assert!(parse_arms("").is_err());
        assert!(parse_baseline("workers=2,3").is_err());
        assert_eq!(plan(None, None, None).unwrap(), None);
        assert!(plan(None, Some(0), None).is_err());
        let p = plan(None, Some(3), None).unwrap().unwrap();
        assert_eq!((p.arms.len(), p.repeat, p.baseline), (0, 3, Arm::default()));
    }

    #[test]
    fn the_diff_names_the_lines_an_arm_saves_loses_and_changes() {
        let long = "You kneel when the Overlord speaks, and you rise only when he lets you go.";
        let texts: BTreeMap<u64, String> = [
            (1, "Kneel.".to_string()),
            (2, long.to_string()),
            (3, "Silence.".to_string()),
        ]
        .into();
        let base = outcomes(&[
            (1, Outcome::Spoken),
            (2, Outcome::Late),
            (3, Outcome::QueueFull),
        ]);
        let arm = outcomes(&[
            (1, Outcome::Spoken),
            (2, Outcome::Spoken),
            (3, Outcome::Error),
        ]);
        let d = compare(&base, &arm, &texts);
        assert_eq!(
            d.saved,
            [LineChange {
                seq: 2,
                from: Outcome::Late,
                to: Outcome::Spoken,
                text: cut(long, 60),
            }]
        );
        assert_eq!(d.saved[0].text.chars().count(), 60);
        assert!(d.saved[0].text.ends_with("..."));
        assert!(d.lost.is_empty());
        assert_eq!(
            d.changed,
            [LineChange {
                seq: 3,
                from: Outcome::QueueFull,
                to: Outcome::Error,
                text: "Silence.".into(),
            }]
        );
        let back = compare(&arm, &base, &texts);
        assert_eq!(back.lost.iter().map(|c| c.seq).collect::<Vec<_>>(), [2]);
    }

    fn report(first_p50: u64, spoken: usize, o: &[(u64, Outcome)]) -> Report {
        Report {
            workers: 3,
            lines_in_flight: 3,
            lines: 3,
            spoken,
            first_audio_ms: Percentiles {
                n: 2,
                p50: first_p50,
                p90: first_p50 + 400,
                max: first_p50 + 500,
            },
            outcomes: outcomes(o),
            ..Report::default()
        }
    }

    #[test]
    fn repeats_aggregate_to_a_median_with_its_range_and_n() {
        let o = [(1, Outcome::Spoken)];
        let rs = [
            report(1600, 22, &o),
            report(5300, 21, &o),
            report(1900, 24, &o),
        ];
        let s = aggregate(&rs);
        assert_eq!(s.first_audio_p50_ms.seconds_cell(), "1.9 s (1.6-5.3, n=3)");
        assert_eq!(s.spoken.count_cell(), "22 (21-24, n=3)");
        assert_eq!(s.repeats, 3);
        // A repeat where no line spoke has no latency to count.
        let mut silent = report(0, 0, &o);
        silent.first_audio_ms = Percentiles::default();
        let s = aggregate(&[report(1600, 2, &o), silent]);
        assert_eq!(s.first_audio_p50_ms.n, 1);
        assert_eq!(Spread::of([]).seconds_cell(), "-");
        // Majority across repeats, ties to the first seen.
        let m = majority(&[
            report(1, 1, &[(1, Outcome::Late), (2, Outcome::Error)]),
            report(1, 1, &[(1, Outcome::Spoken), (2, Outcome::QueueFull)]),
            report(1, 1, &[(1, Outcome::Spoken), (2, Outcome::Late)]),
        ]);
        assert_eq!(m, outcomes(&[(1, Outcome::Spoken), (2, Outcome::Error)]));
    }

    #[tokio::test]
    async fn two_arms_print_as_json_with_outcomes_and_their_diff() {
        let plan = plan(Some("workers=1,3"), Some(3), Some("workers=3"))
            .unwrap()
            .unwrap();
        let base = VoiceConfig {
            lines_in_flight: 2,
            ..VoiceConfig::default()
        };
        let mut calls = Vec::new();
        let grid = run_plan(
            &base,
            &plan,
            |v: VoiceConfig| {
                calls.push(v.render_workers);
                let w = v.render_workers;
                async move {
                    let mut r = report(
                        1600,
                        2,
                        &[
                            (1, Outcome::Spoken),
                            (
                                2,
                                if w == 1 {
                                    Outcome::Late
                                } else {
                                    Outcome::Spoken
                                },
                            ),
                        ],
                    );
                    r.workers = w;
                    r.texts = [(2, "Rise.".to_string())].into();
                    Ok(r)
                }
            },
            false,
        )
        .await
        .unwrap();
        // The baseline is the workers=3 arm: two configs, interleaved by repeat.
        assert_eq!(calls, [3, 1, 3, 1, 3, 1]);
        let j = to_json(&grid);
        assert_eq!(j["baseline"]["workers"], 3);
        assert_eq!(j["baseline"]["repeats"], 3);
        assert!(j["baseline"].get("saved").is_none());
        let arms = j["arms"].as_array().unwrap();
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0]["workers"], 1);
        assert_eq!(arms[0]["lines_in_flight"], 2);
        assert_eq!(arms[0]["repeats"], 3);
        assert_eq!(arms[0]["outcomes"]["1"], "spoken");
        assert_eq!(arms[0]["outcomes"]["2"], "late");
        assert_eq!(arms[0]["saved"], serde_json::json!([]));
        assert_eq!(arms[0]["lost"][0]["seq"], 2);
        assert_eq!(arms[0]["lost"][0]["text"], "Rise.");
        assert_eq!(arms[1]["workers"], 3);
        assert_eq!(arms[1]["lost"], serde_json::json!([]));
        assert_eq!(arms[0]["first_audio_p50_ms"]["n"], 3);
        let t = table(&grid);
        assert!(
            t.contains("baseline: workers 3 flight 2 threads auto"),
            "{t}"
        );
        assert!(
            t.contains("workers 1 flight 2 threads auto vs baseline: saved 0, lost 1, changed 0"),
            "{t}"
        );
        assert!(t.contains("#2"), "{t}");
        assert!(t.contains("\"Rise.\""), "{t}");
    }
}
