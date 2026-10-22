//! The seam between the voice service and whatever renders speech: the sidecar process in the
//! daemon ([`super::sidecar::SidecarRenderer`]), a fake in tests. A renderer takes one line at
//! a time (`say`) and answers with its sentences, in order, on the line's own channel.

use shadowrunr_types::delivery::Delivery;
use shadowrunr_types::voice::{VisemeSpan, VoiceTimings};
use tokio::sync::{mpsc, watch};

/// One line to speak: its text and the delivery cue the brain parsed off it.
#[derive(Debug, Clone, PartialEq)]
pub struct Utter {
    pub text: String,
    pub delivery: Option<Delivery>,
}

impl Utter {
    /// A line with no cue.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            delivery: None,
        }
    }
}

/// One rendered sentence: the sidecar's `audio` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub part: u32,
    pub last: bool,
    pub text: String,
    pub sample_rate: u32,
    /// Mono PCM16LE, base64, as the sidecar sent it.
    pub pcm: String,
    pub visemes: Vec<VisemeSpan>,
    /// Characters sent to the provider for this part (the usage unit).
    pub chars: u32,
    /// Where the part's timing came from.
    pub timings: VoiceTimings,
}

/// What a line's channel carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Audio(Rendered),
    /// The line fails from `part` on (the sidecar's `error`, or the process went away).
    Error {
        part: u32,
        reason: String,
    },
}

/// Whether the renderer can speak.
#[derive(Debug, Clone, PartialEq)]
pub enum RendererState {
    /// Spawned, waiting for `hello` (the first load warms the model).
    Starting,
    /// `hello` arrived. `reason` explains a voice without mouth shapes.
    Ready {
        visemes: bool,
        reason: Option<String>,
    },
    /// Not speaking, and why: no engine, the process exited, the restart cap.
    Degraded(String),
}

pub trait Renderer: Send + Sync + 'static {
    fn state(&self) -> watch::Receiver<RendererState>;
    /// Start rendering `line` as `id`. Its parts arrive in order on the receiver, ending with
    /// a part that is `last` or an error. A renderer that cannot speak answers at once with an
    /// error, so the caller never waits on a dead process.
    fn say(&self, id: &str, line: &Utter) -> mpsc::UnboundedReceiver<Part>;
    /// Stop rendering `id`; nothing more arrives for it.
    fn cancel(&self, id: &str);
}

/// Bytes of PCM in a base64 string, without decoding it.
pub fn b64_len(b64: &str) -> usize {
    let s = b64.trim_end();
    let pad = s.bytes().rev().take_while(|b| *b == b'=').count();
    (s.len() / 4 * 3 + (s.len() % 4).saturating_sub(1)).saturating_sub(pad)
}

/// The length of mono PCM16 at `sample_rate`, in ms.
pub fn duration_ms(pcm_b64: &str, sample_rate: u32) -> u32 {
    if sample_rate == 0 {
        return 0;
    }
    let samples = (b64_len(pcm_b64) / 2) as u64;
    u32::try_from(samples * 1000 / u64::from(sample_rate)).unwrap_or(u32::MAX)
}

/// Standard base64 (with or without padding); `None` on any other byte.
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let s = s.trim().trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
pub mod fake {
    //! A renderer with no process: each line's parts come from a script, so tests drive every
    //! bound without Python.

    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;

    /// How the fake answers one line.
    #[derive(Debug, Clone)]
    pub enum Script {
        /// One sentence per entry, each after the delay.
        Speak(Vec<String>, Duration),
        /// Speak the first `n` sentences, then fail.
        FailAfter(Vec<String>, usize),
        /// Never answer.
        Hang,
    }

    pub struct FakeRenderer {
        state: watch::Sender<RendererState>,
        /// Scripts by line text; anything else speaks its text as one sentence.
        scripts: Mutex<HashMap<String, Script>>,
        pub said: Mutex<Vec<String>>,
        /// Each line's delivery, in the order said.
        pub deliveries: Mutex<Vec<Option<Delivery>>>,
        pub cancelled: Mutex<VecDeque<String>>,
        /// Lines scripted to hang: their senders, held open.
        hung: Mutex<Vec<mpsc::UnboundedSender<Part>>>,
        /// Bytes of PCM in each sentence.
        pub pcm_bytes: usize,
    }

    impl Default for FakeRenderer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl FakeRenderer {
        pub fn new() -> Self {
            Self {
                state: watch::Sender::new(RendererState::Ready {
                    visemes: true,
                    reason: None,
                }),
                scripts: Mutex::new(HashMap::new()),
                said: Mutex::new(Vec::new()),
                deliveries: Mutex::new(Vec::new()),
                cancelled: Mutex::new(VecDeque::new()),
                hung: Mutex::new(Vec::new()),
                pcm_bytes: 4800,
            }
        }

        pub fn with_pcm(mut self, bytes: usize) -> Self {
            self.pcm_bytes = bytes;
            self
        }

        pub fn script(&self, text: &str, s: Script) {
            self.scripts.lock().unwrap().insert(text.into(), s);
        }

        pub fn set_state(&self, s: RendererState) {
            self.state.send_replace(s);
        }

        fn sentence(&self, part: usize, last: bool, text: &str) -> Part {
            // 4800 zero bytes = 100 ms at 24 kHz.
            let pcm = "A".repeat(self.pcm_bytes / 3 * 4);
            Part::Audio(Rendered {
                part: part as u32,
                last,
                text: text.into(),
                sample_rate: 24_000,
                pcm,
                visemes: vec![VisemeSpan {
                    v: shadowrunr_types::voice::Viseme::Aa,
                    start_ms: 0,
                    end_ms: 100,
                }],
                chars: text.chars().count() as u32,
                timings: VoiceTimings::Durations,
            })
        }
    }

    impl Renderer for FakeRenderer {
        fn state(&self) -> watch::Receiver<RendererState> {
            self.state.subscribe()
        }

        fn say(&self, id: &str, line: &Utter) -> mpsc::UnboundedReceiver<Part> {
            let text = line.text.as_str();
            self.said.lock().unwrap().push(id.to_string());
            self.deliveries.lock().unwrap().push(line.delivery);
            let (tx, rx) = mpsc::unbounded_channel();
            let script = self.scripts.lock().unwrap().get(text).cloned();
            let script = script.unwrap_or_else(|| Script::Speak(vec![text.into()], Duration::ZERO));
            let parts: Vec<(Duration, Part)> = match script {
                Script::Speak(s, d) => s
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (d, self.sentence(i, i + 1 == s.len(), t)))
                    .collect(),
                Script::FailAfter(s, n) => {
                    let mut v: Vec<(Duration, Part)> = s
                        .iter()
                        .take(n)
                        .enumerate()
                        .map(|(i, t)| (Duration::ZERO, self.sentence(i, false, t)))
                        .collect();
                    v.push((
                        Duration::ZERO,
                        Part::Error {
                            part: n as u32,
                            reason: "engine exit 1".into(),
                        },
                    ));
                    v
                }
                Script::Hang => {
                    self.hung.lock().unwrap().push(tx);
                    return rx;
                }
            };
            tokio::spawn(async move {
                for (d, p) in parts {
                    if !d.is_zero() {
                        tokio::time::sleep(d).await;
                    }
                    if tx.send(p).is_err() {
                        return;
                    }
                }
            });
            rx
        }

        fn cancel(&self, id: &str) {
            self.cancelled.lock().unwrap().push_back(id.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_lengths_and_decoding() {
        assert_eq!(b64_len(""), 0);
        assert_eq!(b64_len("AAAA"), 3);
        assert_eq!(b64_len("AAA="), 2);
        assert_eq!(b64_len("AA=="), 1);
        assert_eq!(b64_len("AAAAAA"), 4);
        assert_eq!(b64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(b64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(b64_decode("TQ").unwrap(), b"M");
        assert!(b64_decode("T*").is_none());
        // 48000 bytes = 24000 samples = 1 s at 24 kHz.
        let one_second = "A".repeat(64_000);
        assert_eq!(duration_ms(&one_second, 24_000), 1000);
        assert_eq!(duration_ms(&one_second, 0), 0);
    }
}
