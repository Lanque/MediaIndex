use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
    pricing_status: String,
    pricing_checked_at: String,
    pending: Arc<Mutex<Vec<AiUsageEvent>>>,
    budget_gate: Option<AiBudgetGate>,
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
        pricing_status: impl Into<String>,
        pricing_checked_at: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            pricing_status: pricing_status.into(),
            pricing_checked_at: pricing_checked_at.into(),
            pending: Arc::new(Mutex::new(Vec::new())),
            budget_gate: None,
        }
    }

    pub fn with_budget_gate(mut self, budget_gate: AiBudgetGate) -> Self {
        self.budget_gate = Some(budget_gate);
        self
    }

    pub fn try_reserve_request(&self) -> bool {
        self.budget_gate
            .as_ref()
            .is_none_or(AiBudgetGate::try_reserve)
    }

    pub fn record_budget_blocked(&self, operation: &str, model: &str, attempt: u32) {
        let event = AiUsageEvent {
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
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(event);
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
        let event = AiUsageEvent {
            run_id: self.run_id.clone(),
            operation: operation.to_owned(),
            model: model.to_owned(),
            attempt,
            duration_ms,
            outcome: outcome.to_owned(),
            status_code,
            request_id,
            usage_status: "not_reported".to_owned(),
            pricing_status: self.pricing_status.clone(),
            pricing_checked_at: self.pricing_checked_at.clone(),
            reported_input_tokens: None,
            reported_output_tokens: None,
            reported_audio_seconds: None,
            calculated_cost_usd: None,
            possible_cost: true,
        };
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(event);
        }
    }

    pub fn drain(&self) -> Vec<AiUsageEvent> {
        self.pending
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }
}

fn usd_to_micros(value: f64) -> Option<u64> {
    (value.is_finite() && value > 0.0)
        .then(|| (value * 1_000_000.0).ceil())
        .filter(|value| *value <= u64::MAX as f64)
        .map(|value| value as u64)
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
}
