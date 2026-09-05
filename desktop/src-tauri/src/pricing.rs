#[derive(Clone, Copy, Debug)]
pub struct ModelPricing {
    pub input_usd_per_million: Option<f64>,
    pub output_usd_per_million: Option<f64>,
    pub source: &'static str,
}

pub const PRICING_CHECKED_AT: &str = "2026-09-05";
pub const OPENAI_PRICING_SOURCE: &str = "https://developers.openai.com/api/docs/pricing";
pub const OPENAI_EMBEDDING_SOURCE: &str =
    "https://developers.openai.com/api/docs/models/text-embedding-3-small";
pub const GEMINI_PRICING_SOURCE: &str = "https://ai.google.dev/gemini-api/docs/pricing";

pub fn vision_pricing(provider: &str, model: &str) -> Option<ModelPricing> {
    match (provider, model.to_ascii_lowercase().as_str()) {
        ("openai", "gpt-5.6-luna") => Some(ModelPricing {
            input_usd_per_million: Some(0.20),
            output_usd_per_million: Some(1.20),
            source: OPENAI_PRICING_SOURCE,
        }),
        ("openai", "gpt-5.6-terra") => Some(ModelPricing {
            input_usd_per_million: Some(2.00),
            output_usd_per_million: Some(12.00),
            source: OPENAI_PRICING_SOURCE,
        }),
        ("openai", "gpt-5.6-sol") => Some(ModelPricing {
            input_usd_per_million: Some(4.00),
            output_usd_per_million: Some(20.00),
            source: OPENAI_PRICING_SOURCE,
        }),
        ("gemini", "gemini-3.8-flash" | "gemini-3.7-flash") => Some(ModelPricing {
            input_usd_per_million: Some(0.75),
            output_usd_per_million: Some(3.75),
            source: GEMINI_PRICING_SOURCE,
        }),
        _ => None,
    }
}

pub fn embedding_pricing(provider: &str, model: &str) -> Option<f64> {
    match (provider, model.to_ascii_lowercase().as_str()) {
        ("openai", "text-embedding-3-small") => Some(0.02),
        ("gemini", "gemini-embedding-2") => Some(0.20),
        _ => None,
    }
}

pub fn transcription_pricing(provider: &str, model: &str) -> Option<f64> {
    match (provider, model.to_ascii_lowercase().as_str()) {
        ("openai", "whisper-1") => Some(0.006),
        ("openai", "gpt-transcribe") => Some(0.0045),
        ("openai", "gpt-4o-transcribe") => Some(0.006),
        ("openai", "gpt-4o-mini-transcribe") => Some(0.003),
        _ => None,
    }
}

pub fn embedding_source(provider: &str) -> &'static str {
    match provider {
        "openai" => OPENAI_EMBEDDING_SOURCE,
        "gemini" => GEMINI_PRICING_SOURCE,
        _ => "local runtime",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_the_default_openai_whisper_price() {
        assert_eq!(transcription_pricing("openai", "whisper-1"), Some(0.006));
    }
}
