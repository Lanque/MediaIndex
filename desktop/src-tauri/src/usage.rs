use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::pricing;

#[derive(Clone, Debug)]
pub struct AiRunSpec {
    pub run_id: String,
    pub operation: String,
    pub provider: String,
    pub vision_model: String,
    pub embedding_model: String,
    pub transcription_model: Option<String>,
    pub model_namespace: String,
    pub pricing_status: String,
    pub pricing_checked_at: String,
    pub estimated_cost_usd: Option<f64>,
    pub budget_limit_usd: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct AiUsageEvent {
    pub local_event_id: String,
    pub run_id: String,
    pub operation: String,
    pub model: String,
    pub attempt: u32,
    pub duration_ms: u64,
    pub outcome: String,
    pub status_code: Option<u16>,
    pub request_id: Option<String>,
    pub usage_status: String,
    pub pricing_status: String,
    pub pricing_checked_at: String,
    pub reported_input_tokens: Option<u64>,
    pub reported_output_tokens: Option<u64>,
    pub reported_audio_seconds: Option<f64>,
    pub calculated_cost_usd: Option<f64>,
    pub possible_cost: bool,
}

#[derive(Clone, Debug)]
pub struct AiUsageRecorder {
    run_id: String,
    provider: String,
    pricing_status: String,
    pricing_checked_at: String,
    next_event_id: Arc<AtomicU64>,
    state: Arc<Mutex<RecorderState>>,
    budget_gate: Option<AiBudgetGate>,
}

#[derive(Debug, Default)]
struct RecorderState {
    pending: HashMap<String, AiUsageEvent>,
    completed: Vec<AiUsageEvent>,
}

#[derive(Clone, Debug)]
pub struct AiUsageEventHandle {
    local_event_id: String,
    state: Arc<Mutex<RecorderState>>,
}

#[derive(Clone, Debug)]
pub struct AiBudgetGate {
    remaining_micros: Arc<AtomicU64>,
    reserve_micros: u64,
}

impl AiBudgetGate {
    pub fn new(limit_usd: f64, reserve_usd: f64) -> Option<Self> {
        let limit_micros = usd_to_micros(limit_usd)?;
        let reserve_micros = usd_to_micros(reserve_usd)?;
        (reserve_micros > 0).then_some(Self {
            remaining_micros: Arc::new(AtomicU64::new(limit_micros)),
            reserve_micros,
        })
    }

    pub fn try_reserve(&self) -> bool {
        self.remaining_micros
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                if remaining >= self.reserve_micros {
                    Some(remaining - self.reserve_micros)
                } else {
                    None
                }
            })
            .is_ok()
    }

    pub fn reserved_usd(&self, limit_usd: f64) -> f64 {
        let remaining = self.remaining_micros.load(Ordering::Acquire);
        (usd_to_micros(limit_usd)
            .unwrap_or_default()
            .saturating_sub(remaining) as f64)
            / 1_000_000.0
    }
}

impl AiUsageRecorder {
    pub fn new(
        run_id: impl Into<String>,
        provider: impl Into<String>,
        pricing_status: impl Into<String>,
        pricing_checked_at: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            provider: provider.into(),
            pricing_status: pricing_status.into(),
            pricing_checked_at: pricing_checked_at.into(),
            next_event_id: Arc::new(AtomicU64::new(1)),
            state: Arc::new(Mutex::new(RecorderState::default())),
            budget_gate: None,
        }
    }

    pub fn with_budget_gate(mut self, budget_gate: AiBudgetGate) -> Self {
        self.budget_gate = Some(budget_gate);
        self
    }

    pub fn provider_name(&self) -> &str {
        &self.provider
    }

    pub fn try_reserve_request(&self) -> bool {
        self.budget_gate
            .as_ref()
            .is_none_or(AiBudgetGate::try_reserve)
    }

    pub fn record_budget_blocked(&self, operation: &str, model: &str, attempt: u32) {
        let local_event_id = self.new_local_event_id();
        let event = AiUsageEvent {
            local_event_id,
            run_id: self.run_id.clone(),
            operation: operation.to_owned(),
            model: model.to_owned(),
            attempt,
            duration_ms: 0,
            outcome: "budget_blocked".to_owned(),
            status_code: None,
            request_id: None,
            usage_status: "not_sent".to_owned(),
            pricing_status: self.pricing_status.clone(),
            pricing_checked_at: self.pricing_checked_at.clone(),
            reported_input_tokens: None,
            reported_output_tokens: None,
            reported_audio_seconds: None,
            calculated_cost_usd: None,
            possible_cost: false,
        };
        if let Ok(mut state) = self.state.lock() {
            state.completed.push(event);
        }
    }

    pub fn reserved_budget_usd(&self, limit_usd: Option<f64>) -> Option<f64> {
        let limit_usd = limit_usd?;
        Some(
            self.budget_gate
                .as_ref()
                .map(|gate| gate.reserved_usd(limit_usd))
                .unwrap_or_default(),
        )
    }

    pub fn record_request(
        &self,
        operation: &str,
        model: &str,
        attempt: u32,
        duration_ms: u64,
        outcome: &str,
        status_code: Option<u16>,
        request_id: Option<String>,
    ) {
        let handle = self.begin_request(operation, model, attempt);
        handle.record_response(duration_ms, outcome, status_code, request_id);
        handle.finish();
    }

    pub fn begin_request(&self, operation: &str, model: &str, attempt: u32) -> AiUsageEventHandle {
        let local_event_id = self.new_local_event_id();
        let event = AiUsageEvent {
            local_event_id: local_event_id.clone(),
            run_id: self.run_id.clone(),
            operation: operation.to_owned(),
            model: model.to_owned(),
            attempt,
            duration_ms: 0,
            outcome: "started".to_owned(),
            status_code: None,
            request_id: None,
            usage_status: "not_reported".to_owned(),
            pricing_status: self.pricing_status.clone(),
            pricing_checked_at: self.pricing_checked_at.clone(),
            reported_input_tokens: None,
            reported_output_tokens: None,
            reported_audio_seconds: None,
            calculated_cost_usd: None,
            possible_cost: true,
        };
        if let Ok(mut state) = self.state.lock() {
            state.pending.insert(local_event_id.clone(), event);
        }
        AiUsageEventHandle {
            local_event_id,
            state: self.state.clone(),
        }
    }

    pub fn record_reported_usage(
        &self,
        operation: &str,
        model: &str,
        request_id: Option<&str>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        audio_seconds: Option<f64>,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let event = state.pending.values_mut().find(|event| {
            event.operation == operation
                && event.model == model
                && event.usage_status == "not_reported"
                && request_id
                    .is_none_or(|request_id| event.request_id.as_deref() == Some(request_id))
        });
        if let Some(event) = event {
            update_reported_usage(
                event,
                &self.provider,
                input_tokens,
                output_tokens,
                audio_seconds,
            );
        }
    }

    pub fn drain(&self) -> Vec<AiUsageEvent> {
        self.state
            .lock()
            .map(|mut state| std::mem::take(&mut state.completed))
            .unwrap_or_default()
    }

    fn new_local_event_id(&self) -> String {
        format!(
            "{}:{}",
            self.run_id,
            self.next_event_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}

impl AiUsageEventHandle {
    pub fn record_response(
        &self,
        duration_ms: u64,
        outcome: &str,
        status_code: Option<u16>,
        request_id: Option<String>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(event) = state.pending.get_mut(&self.local_event_id) {
                event.duration_ms = duration_ms;
                event.outcome = outcome.to_owned();
                event.status_code = status_code;
                event.request_id = request_id;
            }
        }
    }

    pub fn record_reported_usage(
        &self,
        provider: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        audio_seconds: Option<f64>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(event) = state.pending.get_mut(&self.local_event_id) {
                update_reported_usage(event, provider, input_tokens, output_tokens, audio_seconds);
            }
        }
    }

    pub fn finish(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(event) = state.pending.remove(&self.local_event_id) {
                state.completed.push(event);
            }
        }
    }
}

fn update_reported_usage(
    event: &mut AiUsageEvent,
    provider: &str,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    audio_seconds: Option<f64>,
) {
    event.reported_input_tokens = input_tokens;
    event.reported_output_tokens = output_tokens;
    event.reported_audio_seconds = audio_seconds;
    event.calculated_cost_usd = reported_cost(
        provider,
        &event.operation,
        &event.model,
        input_tokens,
        output_tokens,
    );
    if input_tokens.is_some() || output_tokens.is_some() || audio_seconds.is_some() {
        event.usage_status = "reported".to_owned();
    }
}

fn usd_to_micros(value: f64) -> Option<u64> {
    (value.is_finite() && value > 0.0)
        .then(|| (value * 1_000_000.0).ceil())
        .filter(|value| *value <= u64::MAX as f64)
        .map(|value| value as u64)
}

fn reported_cost(
    provider: &str,
    operation: &str,
    model: &str,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
) -> Option<f64> {
    let input_tokens = input_tokens.unwrap_or_default();
    let output_tokens = output_tokens.unwrap_or_default();
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }
    match operation {
        "OpenAI vision" | "Gemini vision" => {
            let pricing = pricing::vision_pricing(provider, model)?;
            Some(
                input_tokens as f64 * pricing.input_usd_per_million.unwrap_or_default()
                    / 1_000_000.0
                    + output_tokens as f64 * pricing.output_usd_per_million.unwrap_or_default()
                        / 1_000_000.0,
            )
        }
        "OpenAI embedding" | "Gemini embedding" => {
            Some(input_tokens as f64 * pricing::embedding_pricing(provider, model)? / 1_000_000.0)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_gate_allows_only_the_reserved_amount_across_workers() {
        let gate = AiBudgetGate::new(1.0, 0.6).expect("valid budget should create a gate");
        let first = gate.clone();
        let second = gate.clone();
        let handles = [
            std::thread::spawn(move || first.try_reserve()),
            std::thread::spawn(move || second.try_reserve()),
        ];
        let successes = handles
            .into_iter()
            .map(|handle| handle.join().expect("worker should finish"))
            .filter(|reserved| *reserved)
            .count();

        assert_eq!(successes, 1);
        assert!((gate.reserved_usd(1.0) - 0.6).abs() < 0.000_001);
    }

    #[test]
    fn reported_embedding_usage_is_recorded_and_priced() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05");
        let handle = recorder.begin_request("OpenAI embedding", "text-embedding-3-small", 1);
        handle.record_response(
            25,
            "response_received",
            Some(200),
            Some("req_test".to_owned()),
        );
        assert!(recorder.drain().is_empty());
        handle.record_reported_usage(recorder.provider_name(), Some(1_000), None, None);
        handle.finish();

        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "reported");
        assert_eq!(event.reported_input_tokens, Some(1_000));
        assert!(event.calculated_cost_usd.is_some_and(|cost| cost > 0.0));
        assert_eq!(event.local_event_id, "run-test:1");
    }

    #[test]
    fn response_usage_survives_a_concurrent_drain_attempt() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05");
        let handle = recorder.begin_request("OpenAI embedding", "text-embedding-3-small", 1);
        handle.record_response(
            25,
            "response_received",
            Some(200),
            Some("req_test".to_owned()),
        );

        let drained_before_body_parse = recorder.drain();
        assert!(drained_before_body_parse.is_empty());

        handle.record_reported_usage(recorder.provider_name(), Some(1_000), None, None);
        handle.finish();

        let event = recorder
            .drain()
            .pop()
            .expect("completed event should be drainable");
        assert_eq!(event.local_event_id, "run-test:1");
        assert_eq!(event.usage_status, "reported");
    }
}
