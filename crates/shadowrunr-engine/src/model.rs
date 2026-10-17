//! Model ids, effort strings and list prices.

use shadowrunr_types::{Effort, ModelTier, Usage};

/// The Claude model id for a tier. The same ids serve the CLI `--model` flag and the
/// Messages API `model` field.
pub fn model_id(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::Haiku => "claude-haiku-5-5",
        ModelTier::Sonnet => "claude-sonnet-5-5",
        ModelTier::Opus => "claude-opus-5-5",
    }
}

/// The effort level as both the CLI `--effort` flag and the API `output_config.effort`
/// spell it.
pub fn effort_str(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max => "max",
    }
}

/// USD per million tokens: (input, output, cache read, 5-minute cache write).
/// First-party list prices, cached 2026-10-06; Haiku's applies to prompts up to 100K tokens.
pub fn price_per_mtok(tier: ModelTier) -> (f64, f64, f64, f64) {
    match tier {
        ModelTier::Haiku => (0.10, 0.50, 0.01, 0.125),
        ModelTier::Sonnet => (2.00, 10.00, 0.20, 2.50),
        ModelTier::Opus => (4.00, 20.00, 0.20, 5.00),
    }
}

/// Estimated cost of one API call from its usage. The CLI reports its own
/// `total_cost_usd`; this is only for the Messages API path, which does not.
pub fn estimate_cost_usd(tier: ModelTier, usage: &Usage) -> f64 {
    let (inp, out, read, write) = price_per_mtok(tier);
    let m = 1_000_000.0;
    usage.input_tokens as f64 * inp / m
        + usage.output_tokens as f64 * out / m
        + usage.cache_read_tokens as f64 * read / m
        + usage.cache_creation_tokens as f64 * write / m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_cost() {
        assert_eq!(model_id(ModelTier::Haiku), "claude-haiku-5-5");
        assert_eq!(model_id(ModelTier::Sonnet), "claude-sonnet-5-5");
        assert_eq!(model_id(ModelTier::Opus), "claude-opus-5-5");
        assert_eq!(effort_str(Effort::Xhigh), "xhigh");
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };
        assert!((estimate_cost_usd(ModelTier::Haiku, &u) - 0.60).abs() < 1e-9);
    }
}
