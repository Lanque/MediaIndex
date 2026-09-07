use serde::Serialize;

use crate::pricing;

const VISION_PROMPT_TOKENS_PER_REQUEST: u64 = 2_000;
const EMBEDDING_TOKENS_PER_FRAME: u64 = 256;
const OUTPUT_TOKENS_PER_FRAME: u64 = 320;
const MAX_IMAGE_SIDE: u32 = 2_048;
const IMAGE_PATCH_SIZE: u32 = 32;
const IMAGE_TOKEN_MULTIPLIER: f64 = 1.2;

#[derive(Clone, Debug)]
pub struct CostFile {
    pub sampled_frames: u64,
    pub vision_requests: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct CostInput {
    pub provider: String,
    pub vision_model: String,
    pub embedding_model: String,
    pub transcription_model: String,
    pub transcribes_audio: bool,
    pub audio_seconds: u64,
    pub budget_limit_usd: Option<f64>,
    pub files: Vec<CostFile>,
}

#[derive(Debug, Serialize)]
pub struct TokenEstimate {
    pub low: u64,
    pub likely: u64,
    pub high: u64,
}

#[derive(Debug, Serialize)]
pub struct AiCostEstimate {
    pub currency: &'static str,
    pub pricing_status: &'static str,
    pub estimated_low_usd: Option<f64>,
    pub estimated_likely_usd: Option<f64>,
    pub estimated_high_usd: Option<f64>,
    pub vision_input_tokens: TokenEstimate,
    pub vision_output_tokens: TokenEstimate,
    pub embedding_input_tokens: TokenEstimate,
    pub audio_seconds: u64,
    pub pricing_checked_at: &'static str,
    pub pricing_source: &'static str,
    pub budget_limit_usd: Option<f64>,
    pub budget_status: &'static str,
    pub assumptions: Vec<String>,
}

pub fn estimate(input: CostInput) -> AiCostEstimate {
    let frame_count = input
        .files
        .iter()
        .map(|file| file.sampled_frames)
        .sum::<u64>();
    let vision_requests = input
        .files
        .iter()
        .map(|file| file.vision_requests)
        .sum::<u64>();
    let image_tokens_likely = input
        .files
        .iter()
        .map(|file| image_tokens(file.width, file.height).saturating_mul(file.sampled_frames))
        .sum::<u64>();
    let prompt_tokens = vision_requests.saturating_mul(VISION_PROMPT_TOKENS_PER_REQUEST);
    let vision_input_likely = image_tokens_likely.saturating_add(prompt_tokens);
    let vision_input_tokens = TokenEstimate {
        low: scale_tokens(vision_input_likely, 3, 4),
        likely: vision_input_likely,
        high: scale_tokens(vision_input_likely, 5, 4),
    };
    let vision_output_tokens = TokenEstimate {
        low: frame_count.saturating_mul(OUTPUT_TOKENS_PER_FRAME / 2),
        likely: frame_count.saturating_mul(OUTPUT_TOKENS_PER_FRAME),
        high: frame_count.saturating_mul(OUTPUT_TOKENS_PER_FRAME.saturating_mul(2)),
    };
    let embedding_input_tokens = TokenEstimate {
        low: frame_count.saturating_mul(EMBEDDING_TOKENS_PER_FRAME / 2),
        likely: frame_count.saturating_mul(EMBEDDING_TOKENS_PER_FRAME),
        high: frame_count.saturating_mul(EMBEDDING_TOKENS_PER_FRAME.saturating_mul(2)),
    };

    if input.provider == "local" {
        return AiCostEstimate {
            currency: "USD",
            pricing_status: "local",
            estimated_low_usd: None,
            estimated_likely_usd: None,
            estimated_high_usd: None,
            vision_input_tokens,
            vision_output_tokens,
            embedding_input_tokens,
            audio_seconds: input.audio_seconds,
            pricing_checked_at: pricing::PRICING_CHECKED_AT,
            pricing_source: "local runtime",
            budget_limit_usd: input.budget_limit_usd,
            budget_status: "local",
            assumptions: vec![
                "No provider API fee is included; local CPU/GPU time and electricity are separate."
                    .to_owned(),
            ],
        };
    }

    let vision = pricing::vision_pricing(&input.provider, &input.vision_model);
    let embedding_rate = pricing::embedding_pricing(&input.provider, &input.embedding_model);
    let transcription_rate = if input.transcribes_audio && input.audio_seconds > 0 {
        pricing::transcription_pricing(&input.provider, &input.transcription_model)
    } else {
        Some(0.0)
    };
    let source = vision
        .map(|pricing| pricing.source)
        .unwrap_or_else(|| pricing::embedding_source(&input.provider));
    let assumptions = assumptions(
        &input,
        vision.is_some(),
        embedding_rate.is_some(),
        transcription_rate.is_some(),
    );
    let pricing_known =
        vision.is_some() && embedding_rate.is_some() && transcription_rate.is_some();
    let computed_budget_status = budget_status(pricing_known, None, input.budget_limit_usd);

    let Some(vision) = vision else {
        return unknown_estimate(
            vision_input_tokens,
            vision_output_tokens,
            embedding_input_tokens,
            input.audio_seconds,
            source,
            input.budget_limit_usd,
            computed_budget_status,
            assumptions,
        );
    };
    let Some(embedding_rate) = embedding_rate else {
        return unknown_estimate(
            vision_input_tokens,
            vision_output_tokens,
            embedding_input_tokens,
            input.audio_seconds,
            source,
            input.budget_limit_usd,
            computed_budget_status,
            assumptions,
        );
    };
    let Some(transcription_rate) = transcription_rate else {
        return unknown_estimate(
            vision_input_tokens,
            vision_output_tokens,
            embedding_input_tokens,
            input.audio_seconds,
            source,
            input.budget_limit_usd,
            computed_budget_status,
            assumptions,
        );
    };

    let vision_cost = |input_tokens: u64, output_tokens: u64| {
        input_tokens as f64 * vision.input_usd_per_million.unwrap_or_default() / 1_000_000.0
            + output_tokens as f64 * vision.output_usd_per_million.unwrap_or_default() / 1_000_000.0
    };
    let embedding_cost =
        |embedding_tokens: u64| embedding_tokens as f64 * embedding_rate / 1_000_000.0;
    let transcription_cost = input.audio_seconds as f64 / 60.0 * transcription_rate;
    let cost = |input_tokens: u64, output_tokens: u64, embedding_tokens: u64| {
        vision_cost(input_tokens, output_tokens)
            + embedding_cost(embedding_tokens)
            + transcription_cost
    };
    let estimated_high_usd = cost(
        vision_input_tokens.high,
        vision_output_tokens.high,
        embedding_input_tokens.high,
    );

    AiCostEstimate {
        currency: "USD",
        pricing_status: "known",
        estimated_low_usd: Some(cost(
            vision_input_tokens.low,
            vision_output_tokens.low,
            embedding_input_tokens.low,
        )),
        estimated_likely_usd: Some(cost(
            vision_input_tokens.likely,
            vision_output_tokens.likely,
            embedding_input_tokens.likely,
        )),
        estimated_high_usd: Some(cost(
            vision_input_tokens.high,
            vision_output_tokens.high,
            embedding_input_tokens.high,
        )),
        vision_input_tokens,
        vision_output_tokens,
        embedding_input_tokens,
        audio_seconds: input.audio_seconds,
        pricing_checked_at: pricing::PRICING_CHECKED_AT,
        pricing_source: source,
        budget_limit_usd: input.budget_limit_usd,
        budget_status: budget_status(true, Some(estimated_high_usd), input.budget_limit_usd),
        assumptions,
    }
}

fn unknown_estimate(
    vision_input_tokens: TokenEstimate,
    vision_output_tokens: TokenEstimate,
    embedding_input_tokens: TokenEstimate,
    audio_seconds: u64,
    source: &'static str,
    budget_limit_usd: Option<f64>,
    budget_status: &'static str,
    assumptions: Vec<String>,
) -> AiCostEstimate {
    AiCostEstimate {
        currency: "USD",
        pricing_status: "unknown",
        estimated_low_usd: None,
        estimated_likely_usd: None,
        estimated_high_usd: None,
        vision_input_tokens,
        vision_output_tokens,
        embedding_input_tokens,
        audio_seconds,
        pricing_checked_at: pricing::PRICING_CHECKED_AT,
        pricing_source: source,
        budget_limit_usd,
        budget_status,
        assumptions,
    }
}

pub fn vision_request_cost(
    provider: &str,
    model: &str,
    image_dimensions: &[ImageDimensions],
    prompt_tokens_upper: u64,
    max_output_tokens: u64,
) -> Option<f64> {
    let pricing = pricing::vision_pricing(provider, model)?;
    let image_tokens = image_dimensions
        .iter()
        .map(|dimensions| image_tokens(Some(dimensions.width), Some(dimensions.height)))
        .sum::<u64>();
    Some(
        token_cost(
            image_tokens.saturating_add(prompt_tokens_upper),
            pricing.input_usd_per_million?,
        ) + token_cost(max_output_tokens, pricing.output_usd_per_million?),
    )
}

pub fn embedding_request_cost(provider: &str, model: &str, input_tokens_upper: u64) -> Option<f64> {
    Some(token_cost(
        input_tokens_upper,
        pricing::embedding_pricing(provider, model)?,
    ))
}

pub fn transcription_request_cost(provider: &str, model: &str, audio_seconds: f64) -> Option<f64> {
    if !audio_seconds.is_finite() || audio_seconds < 0.0 {
        return None;
    }
    Some(audio_seconds / 60.0 * pricing::transcription_pricing(provider, model)?)
}

pub fn text_tokens_upper(text: &str) -> u64 {
    text.len() as u64
}

fn token_cost(tokens: u64, usd_per_million: f64) -> f64 {
    tokens as f64 * usd_per_million / 1_000_000.0
}

fn assumptions(
    input: &CostInput,
    vision_known: bool,
    embedding_known: bool,
    transcription_known: bool,
) -> Vec<String> {
    let missing_dimensions = input
        .files
        .iter()
        .any(|file| file.width.is_none() || file.height.is_none());
    let mut assumptions = vec![
        "Vision input uses a high-detail image-token estimate based on frame dimensions."
            .to_owned(),
        "Prompt, JSON schema, output, retry reserve, taxes, and provider account credits can change the final bill."
            .to_owned(),
    ];
    if missing_dimensions {
        assumptions.push(
            "Some frame dimensions are unknown; 1280x720 is used for those frames.".to_owned(),
        );
    }
    if !vision_known {
        assumptions.push(format!(
            "No checked price is available for vision model '{}'.",
            input.vision_model
        ));
    }
    if !embedding_known {
        assumptions.push(format!(
            "No checked price is available for embedding model '{}'.",
            input.embedding_model
        ));
    }
    if input.transcribes_audio && input.audio_seconds > 0 && !transcription_known {
        assumptions.push(format!(
            "No checked price is available for transcription model '{}'.",
            input.transcription_model
        ));
    }
    assumptions
}

fn budget_status(
    pricing_known: bool,
    estimated_high_usd: Option<f64>,
    budget_limit_usd: Option<f64>,
) -> &'static str {
    let Some(limit) = budget_limit_usd else {
        return "not_configured";
    };
    if !pricing_known {
        return "unknown";
    }
    if estimated_high_usd.is_some_and(|estimate| estimate <= limit) {
        "within_limit"
    } else {
        "exceeds_limit"
    }
}

fn image_tokens(width: Option<u32>, height: Option<u32>) -> u64 {
    let (width, height) = (width.unwrap_or(1_280), height.unwrap_or(720));
    let scale = (MAX_IMAGE_SIDE as f64 / u32::max(width, height) as f64).min(1.0);
    let resized_width = (width as f64 * scale).ceil() as u32;
    let resized_height = (height as f64 * scale).ceil() as u32;
    let patches = u64::from(resized_width.div_ceil(IMAGE_PATCH_SIZE))
        .saturating_mul(u64::from(resized_height.div_ceil(IMAGE_PATCH_SIZE)));
    (patches as f64 * IMAGE_TOKEN_MULTIPLIER).ceil() as u64
}

fn scale_tokens(value: u64, numerator: u64, denominator: u64) -> u64 {
    value.saturating_mul(numerator).div_ceil(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(provider: &str) -> CostInput {
        CostInput {
            provider: provider.to_owned(),
            vision_model: if provider == "openai" {
                "gpt-5.6-luna".to_owned()
            } else {
                "gemini-3.8-flash".to_owned()
            },
            embedding_model: if provider == "openai" {
                "text-embedding-3-small".to_owned()
            } else {
                "gemini-embedding-2".to_owned()
            },
            transcription_model: "whisper-1".to_owned(),
            transcribes_audio: false,
            audio_seconds: 0,
            budget_limit_usd: None,
            files: vec![CostFile {
                sampled_frames: 8,
                vision_requests: 1,
                width: Some(1_280),
                height: Some(720),
            }],
        }
    }

    #[test]
    fn calculates_a_non_zero_known_remote_estimate() {
        let estimate = estimate(input("openai"));

        assert_eq!(estimate.pricing_status, "known");
        assert!(estimate.estimated_low_usd.unwrap() > 0.0);
        assert!(estimate.estimated_low_usd <= estimate.estimated_likely_usd);
        assert!(estimate.estimated_likely_usd <= estimate.estimated_high_usd);
        assert!(estimate.vision_input_tokens.likely > 0);
        assert!(estimate.embedding_input_tokens.likely > 0);
    }

    #[test]
    fn unknown_pricing_never_becomes_zero() {
        let mut input = input("openai");
        input.vision_model = "custom-vision".to_owned();

        let estimate = estimate(input);

        assert_eq!(estimate.pricing_status, "unknown");
        assert!(estimate.estimated_low_usd.is_none());
        assert!(estimate
            .assumptions
            .iter()
            .any(|assumption| assumption.contains("custom-vision")));
    }

    #[test]
    fn local_estimate_exposes_api_fee_boundary() {
        let estimate = estimate(input("local"));

        assert_eq!(estimate.pricing_status, "local");
        assert!(estimate.estimated_likely_usd.is_none());
        assert!(estimate.assumptions[0].contains("local CPU/GPU"));
    }

    #[test]
    fn budget_status_is_conservative_and_unknown_is_not_approved() {
        let mut known_input = input("openai");
        known_input.budget_limit_usd = Some(1.0);
        assert_eq!(estimate(known_input).budget_status, "within_limit");

        let mut unknown_input = input("openai");
        unknown_input.vision_model = "custom-vision".to_owned();
        unknown_input.budget_limit_usd = Some(1.0);
        assert_eq!(estimate(unknown_input).budget_status, "unknown");
    }

    #[test]
    fn request_cost_uses_the_actual_vision_payload_size() {
        let one_frame = vision_request_cost(
            "openai",
            "gpt-5.6-luna",
            &[ImageDimensions {
                width: 1_280,
                height: 720,
            }],
            2_000,
            1_280,
        )
        .expect("vision price should be known");
        let eight_frames = vision_request_cost(
            "openai",
            "gpt-5.6-luna",
            &[ImageDimensions {
                width: 1_280,
                height: 720,
            }; 8],
            2_000,
            2_560,
        )
        .expect("vision price should be known");

        assert!(eight_frames > one_frame);
    }

    #[test]
    fn request_cost_uses_the_actual_embedding_payload_size() {
        let short = embedding_request_cost("openai", "text-embedding-3-small", 32)
            .expect("embedding price should be known");
        let long = embedding_request_cost("openai", "text-embedding-3-small", 256)
            .expect("embedding price should be known");

        assert!(long > short);
        assert!((long / short - 8.0).abs() < 0.000_001);
    }

    #[test]
    fn default_whisper_audio_is_budgetable() {
        let mut input = input("openai");
        input.transcribes_audio = true;
        input.audio_seconds = 60;

        let estimate = estimate(input);

        assert_eq!(estimate.pricing_status, "known");
        assert_eq!(
            transcription_request_cost("openai", "whisper-1", 60.0),
            Some(0.006)
        );
    }
}
