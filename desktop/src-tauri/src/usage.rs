use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    pub estimated_audio_seconds: Option<f64>,
    pub calculated_cost_usd: Option<f64>,
    pub reserved_cost_usd: Option<f64>,
    pub budget_adjustment_usd: Option<f64>,
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
    budget_gate: Option<AiBudgetGate>,
    reserved_micros: Option<u64>,
    reservation_adjusted: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub struct AiBudgetGate {
    state: Arc<Mutex<BudgetState>>,
}

#[derive(Debug)]
struct BudgetState {
    limit_micros: u64,
    settled_micros: u64,
    uncertain_micros: u64,
    reservations: HashMap<String, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UsageAssessment {
    calculated_cost_usd: Option<f64>,
    lower_bound_usd: Option<f64>,
    usage_status: &'static str,
}

impl AiBudgetGate {
    pub fn new(limit_usd: f64) -> Option<Self> {
        let limit_micros = usd_to_micros(limit_usd)?;
        Some(Self {
            state: Arc::new(Mutex::new(BudgetState {
                limit_micros,
                settled_micros: 0,
                uncertain_micros: 0,
                reservations: HashMap::new(),
            })),
        })
    }

    pub fn try_reserve(&self, local_event_id: &str, reserve_usd: f64) -> Option<u64> {
        let reserve_micros = usd_to_nonnegative_micros(reserve_usd)?;
        let mut state = self.state.lock().ok()?;
        if state.reservations.contains_key(local_event_id) {
            return None;
        }
        let active_micros = state
            .reservations
            .values()
            .copied()
            .fold(0u64, u64::saturating_add);
        let committed_micros = state
            .settled_micros
            .saturating_add(state.uncertain_micros)
            .saturating_add(active_micros);
        if committed_micros.saturating_add(reserve_micros) > state.limit_micros {
            return None;
        }
        state
            .reservations
            .insert(local_event_id.to_owned(), reserve_micros);
        Some(reserve_micros)
    }

    pub fn settle(&self, local_event_id: &str, actual_usd: Option<f64>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(reserved_micros) = state.reservations.remove(local_event_id) else {
            return;
        };
        if let Some(actual_usd) = actual_usd {
            state.settled_micros = state
                .settled_micros
                .saturating_add(usd_to_nonnegative_micros(actual_usd).unwrap_or(reserved_micros));
        } else {
            state.uncertain_micros = state.uncertain_micros.saturating_add(reserved_micros);
        }
    }

    pub fn reserved_usd(&self, _limit_usd: f64) -> f64 {
        let Ok(state) = self.state.lock() else {
            return 0.0;
        };
        let active_micros = state
            .reservations
            .values()
            .copied()
            .fold(0u64, u64::saturating_add);
        state
            .settled_micros
            .saturating_add(state.uncertain_micros)
            .saturating_add(active_micros) as f64
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
            estimated_audio_seconds: None,
            calculated_cost_usd: None,
            reserved_cost_usd: None,
            budget_adjustment_usd: None,
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
        self.begin_request_with_reservation(
            local_event_id,
            operation,
            model,
            attempt,
            None,
            None,
            None,
        )
    }

    pub fn begin_reserved_request(
        &self,
        operation: &str,
        model: &str,
        attempt: u32,
        reserve_usd: Option<f64>,
    ) -> Option<AiUsageEventHandle> {
        let local_event_id = self.new_local_event_id();
        let (budget_gate, reserved_micros, reserved_cost_usd) =
            if let Some(budget_gate) = self.budget_gate.as_ref() {
                let reserve_usd = reserve_usd?;
                let reserved_micros = budget_gate.try_reserve(&local_event_id, reserve_usd)?;
                (
                    Some(budget_gate.clone()),
                    Some(reserved_micros),
                    Some(reserve_usd),
                )
            } else {
                (None, None, None)
            };
        Some(self.begin_request_with_reservation(
            local_event_id,
            operation,
            model,
            attempt,
            budget_gate,
            reserved_micros,
            reserved_cost_usd,
        ))
    }

    fn begin_request_with_reservation(
        &self,
        local_event_id: String,
        operation: &str,
        model: &str,
        attempt: u32,
        budget_gate: Option<AiBudgetGate>,
        reserved_micros: Option<u64>,
        reserved_cost_usd: Option<f64>,
    ) -> AiUsageEventHandle {
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
            estimated_audio_seconds: None,
            calculated_cost_usd: None,
            reserved_cost_usd,
            budget_adjustment_usd: None,
            possible_cost: true,
        };
        if let Ok(mut state) = self.state.lock() {
            state.pending.insert(local_event_id.clone(), event);
        }
        AiUsageEventHandle {
            local_event_id,
            state: self.state.clone(),
            budget_gate,
            reserved_micros,
            reservation_adjusted: Arc::new(AtomicBool::new(false)),
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
                && matches!(
                    event.usage_status.as_str(),
                    "not_reported" | "partial" | "estimated"
                )
                && request_id
                    .is_none_or(|request_id| event.request_id.as_deref() == Some(request_id))
        });
        if let Some(event) = event {
            let _ = update_reported_usage(
                event,
                &self.provider,
                input_tokens,
                output_tokens,
                audio_seconds,
                None,
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
        estimated_audio_seconds: Option<f64>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(event) = state.pending.get_mut(&self.local_event_id) {
                let assessment = update_reported_usage(
                    event,
                    provider,
                    input_tokens,
                    output_tokens,
                    audio_seconds,
                    estimated_audio_seconds,
                );
                let settlement_cost_usd = assessment.calculated_cost_usd.or_else(|| {
                    let lower_bound_usd = assessment.lower_bound_usd?;
                    let lower_bound_micros = usd_to_nonnegative_micros(lower_bound_usd)?;
                    (lower_bound_micros > self.reserved_micros?)
                        .then(|| lower_bound_micros as f64 / 1_000_000.0)
                });
                if let (Some(budget_gate), Some(actual_cost_usd)) =
                    (self.budget_gate.as_ref(), settlement_cost_usd)
                {
                    if !self.reservation_adjusted.swap(true, Ordering::AcqRel) {
                        budget_gate.settle(&self.local_event_id, Some(actual_cost_usd));
                        event.budget_adjustment_usd = event
                            .reserved_cost_usd
                            .map(|reserved| actual_cost_usd - reserved);
                    }
                }
            }
        }
    }

    pub fn finish(&self) {
        if let Some(budget_gate) = self.budget_gate.as_ref() {
            if !self.reservation_adjusted.swap(true, Ordering::AcqRel) {
                budget_gate.settle(&self.local_event_id, None);
            }
        }
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
    estimated_audio_seconds: Option<f64>,
) -> UsageAssessment {
    let input_tokens = input_tokens.or(event.reported_input_tokens);
    let output_tokens = output_tokens.or(event.reported_output_tokens);
    let audio_seconds = valid_audio_seconds(audio_seconds).or(event.reported_audio_seconds);
    let estimated_audio_seconds =
        valid_audio_seconds(estimated_audio_seconds).or(event.estimated_audio_seconds);
    event.reported_input_tokens = input_tokens;
    event.reported_output_tokens = output_tokens;
    event.reported_audio_seconds = audio_seconds;
    event.estimated_audio_seconds = estimated_audio_seconds;
    let assessment = reported_cost(
        provider,
        &event.operation,
        &event.model,
        input_tokens,
        output_tokens,
        audio_seconds,
        estimated_audio_seconds,
    );
    event.calculated_cost_usd = assessment.calculated_cost_usd;
    event.usage_status = assessment.usage_status.to_owned();
    assessment
}

fn usd_to_micros(value: f64) -> Option<u64> {
    usd_to_nonnegative_micros(value).filter(|value| *value > 0)
}

fn usd_to_nonnegative_micros(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0)
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
    reported_audio_seconds: Option<f64>,
    estimated_audio_seconds: Option<f64>,
) -> UsageAssessment {
    match operation {
        "OpenAI vision" | "Gemini vision" => {
            let pricing = pricing::vision_pricing(provider, model);
            let input_cost = input_tokens
                .zip(pricing.and_then(|pricing| pricing.input_usd_per_million))
                .map(|(tokens, rate)| tokens as f64 * rate / 1_000_000.0);
            let output_cost = output_tokens
                .zip(pricing.and_then(|pricing| pricing.output_usd_per_million))
                .map(|(tokens, rate)| tokens as f64 * rate / 1_000_000.0);
            UsageAssessment {
                calculated_cost_usd: input_tokens.zip(output_tokens).and_then(|_| {
                    input_cost
                        .zip(output_cost)
                        .map(|(input, output)| input + output)
                }),
                lower_bound_usd: add_known_costs(input_cost, output_cost),
                usage_status: usage_status_for_tokens(
                    input_tokens.is_some() && output_tokens.is_some(),
                    input_tokens.is_some() || output_tokens.is_some(),
                ),
            }
        }
        "OpenAI embedding" | "Gemini embedding" => {
            let input_cost = input_tokens
                .zip(pricing::embedding_pricing(provider, model))
                .map(|(tokens, rate)| tokens as f64 * rate / 1_000_000.0);
            UsageAssessment {
                calculated_cost_usd: input_cost,
                lower_bound_usd: input_cost,
                usage_status: usage_status_for_tokens(
                    input_tokens.is_some(),
                    input_tokens.is_some() || output_tokens.is_some(),
                ),
            }
        }
        "OpenAI speech transcription" => {
            let audio_seconds = reported_audio_seconds.or(estimated_audio_seconds);
            let audio_cost = audio_seconds
                .zip(pricing::transcription_pricing(provider, model))
                .map(|(seconds, rate)| seconds / 60.0 * rate);
            UsageAssessment {
                calculated_cost_usd: audio_cost,
                lower_bound_usd: audio_cost,
                usage_status: if reported_audio_seconds.is_some() {
                    "reported"
                } else if estimated_audio_seconds.is_some() {
                    "estimated"
                } else if input_tokens.is_some() || output_tokens.is_some() {
                    "partial"
                } else {
                    "not_reported"
                },
            }
        }
        _ => UsageAssessment {
            calculated_cost_usd: None,
            lower_bound_usd: None,
            usage_status: "not_reported",
        },
    }
}

fn valid_audio_seconds(value: Option<f64>) -> Option<f64> {
    value.filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}

fn add_known_costs(first: Option<f64>, second: Option<f64>) -> Option<f64> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first + second),
        (Some(cost), None) | (None, Some(cost)) => Some(cost),
        (None, None) => None,
    }
}

fn usage_status_for_tokens(complete: bool, any: bool) -> &'static str {
    if complete {
        "reported"
    } else if any {
        "partial"
    } else {
        "not_reported"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_gate_allows_only_the_reserved_amount_across_workers() {
        let gate = AiBudgetGate::new(1.0).expect("valid budget should create a gate");
        let first = gate.clone();
        let second = gate.clone();
        let handles = [
            std::thread::spawn(move || first.try_reserve("run-test:1", 0.6).is_some()),
            std::thread::spawn(move || second.try_reserve("run-test:2", 0.6).is_some()),
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
    fn budget_deficit_is_not_hidden_when_reservations_settle_in_either_order() {
        for settle_first in [true, false] {
            let gate = AiBudgetGate::new(1.0).expect("valid budget should create a gate");
            assert!(gate.try_reserve("run-test:1", 0.5).is_some());
            assert!(gate.try_reserve("run-test:2", 0.5).is_some());

            if settle_first {
                gate.settle("run-test:1", Some(1.2));
                gate.settle("run-test:2", Some(0.1));
            } else {
                gate.settle("run-test:2", Some(0.1));
                gate.settle("run-test:1", Some(1.2));
            }

            assert!(gate.try_reserve("run-test:3", 0.01).is_none());
            assert!(gate.reserved_usd(1.0) > 1.0);
        }
    }

    #[test]
    fn settling_the_same_request_twice_does_not_release_extra_budget() {
        let gate = AiBudgetGate::new(1.0).expect("valid budget should create a gate");
        assert!(gate.try_reserve("run-test:1", 0.5).is_some());
        gate.settle("run-test:1", Some(0.1));
        gate.settle("run-test:1", Some(0.1));

        assert!(gate.try_reserve("run-test:2", 0.9).is_some());
    }

    #[test]
    fn reported_usage_releases_the_unused_request_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI embedding", "text-embedding-3-small", 1, Some(0.6))
            .expect("request should reserve its type-specific amount");

        handle.record_reported_usage(recorder.provider_name(), Some(1_000), None, None, None);

        assert!(recorder.reserved_budget_usd(Some(1.0)).unwrap() < 0.6);
        assert!(handle.reserved_micros.is_some_and(|reserved| reserved > 0));
        handle.finish();
    }

    #[test]
    fn missing_vision_usage_keeps_the_entire_request_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI vision", "gpt-5.6-luna", 1, Some(0.6))
            .expect("vision request should reserve its cost");

        handle.record_reported_usage(recorder.provider_name(), None, None, None, None);
        handle.finish();

        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "not_reported");
        assert_eq!(event.calculated_cost_usd, None);
    }

    #[test]
    fn partial_vision_usage_keeps_the_entire_request_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI vision", "gpt-5.6-luna", 1, Some(0.6))
            .expect("vision request should reserve its cost");

        handle.record_reported_usage(recorder.provider_name(), Some(1_000), None, None, None);
        handle.finish();

        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "partial");
        assert_eq!(event.calculated_cost_usd, None);
    }

    #[test]
    fn missing_embedding_input_keeps_the_entire_request_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI embedding", "text-embedding-3-small", 1, Some(0.6))
            .expect("embedding request should reserve its cost");

        handle.record_reported_usage(recorder.provider_name(), None, Some(1_000), None, None);
        handle.finish();

        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "partial");
        assert_eq!(event.calculated_cost_usd, None);
    }

    #[test]
    fn complete_zero_vision_usage_releases_the_request_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI vision", "gpt-5.6-luna", 1, Some(0.6))
            .expect("vision request should reserve its cost");

        handle.record_reported_usage(recorder.provider_name(), Some(0), Some(0), None, None);
        handle.finish();

        assert_eq!(recorder.reserved_budget_usd(Some(1.0)), Some(0.0));
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "reported");
        assert_eq!(event.calculated_cost_usd, Some(0.0));
    }

    #[test]
    fn partial_usage_charges_a_known_lower_bound_when_it_exceeds_the_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(10.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI vision", "gpt-5.6-luna", 1, Some(0.01))
            .expect("vision request should reserve its cost");

        handle.record_reported_usage(
            recorder.provider_name(),
            Some(100_000_000),
            None,
            None,
            None,
        );
        handle.finish();

        assert!(recorder.reserved_budget_usd(Some(10.0)).unwrap() > 10.0);
        assert!(recorder
            .begin_reserved_request("OpenAI embedding", "text-embedding-3-small", 1, Some(0.01))
            .is_none());
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "partial");
        assert!(event
            .budget_adjustment_usd
            .is_some_and(|adjustment| adjustment > 0.0));
    }

    #[test]
    fn local_audio_duration_is_kept_as_an_estimate_for_budgeting() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI speech transcription", "whisper-1", 1, Some(0.006))
            .expect("speech request should reserve its measured duration");

        handle.record_reported_usage(recorder.provider_name(), None, None, None, Some(60.0));
        handle.finish();

        let event = recorder
            .drain()
            .pop()
            .expect("speech event should be drainable");
        assert_eq!(event.usage_status, "estimated");
        assert_eq!(event.reported_audio_seconds, None);
        assert_eq!(event.estimated_audio_seconds, Some(60.0));
        assert!(event.calculated_cost_usd.is_some_and(|cost| cost > 0.0));
        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.006).abs() < 0.000_001);
    }

    #[test]
    fn missing_audio_duration_keeps_the_entire_request_reserve() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let handle = recorder
            .begin_reserved_request("OpenAI speech transcription", "whisper-1", 1, Some(0.006))
            .expect("speech request should reserve its cost");

        handle.record_reported_usage(recorder.provider_name(), None, None, None, None);
        handle.finish();

        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.006).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "not_reported");
        assert_eq!(event.calculated_cost_usd, None);
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
        handle.record_reported_usage(recorder.provider_name(), Some(1_000), None, None, None);
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

        handle.record_reported_usage(recorder.provider_name(), Some(1_000), None, None, None);
        handle.finish();

        let event = recorder
            .drain()
            .pop()
            .expect("completed event should be drainable");
        assert_eq!(event.local_event_id, "run-test:1");
        assert_eq!(event.usage_status, "reported");
    }
}
