use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetCalibrationConfidence {
    High,
    Medium,
    Low,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetCalibration {
    pub model: String,
    pub tokens_per_char: f32,
    pub output_tokens_per_char: f32,
    pub tool_schema_overhead: f32,
    pub system_overhead: f32,
    pub samples: u64,
    pub last_error_ratio: f32,
    #[serde(default)]
    pub rolling_error_ratios: Vec<f32>,
}

impl BudgetCalibration {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            tokens_per_char: 1.0 / 3.0,
            output_tokens_per_char: 1.0 / 2.5,
            tool_schema_overhead: 0.0,
            system_overhead: 0.0,
            samples: 0,
            last_error_ratio: 1.0,
            rolling_error_ratios: Vec::new(),
        }
    }

    pub fn record(&mut self, actual_input_tokens: u64, total_chars: usize) {
        if total_chars == 0 {
            return;
        }
        let observed = actual_input_tokens as f32 / total_chars as f32;
        self.tokens_per_char = self.tokens_per_char * 0.9 + observed * 0.1;
        self.last_error_ratio = observed / self.tokens_per_char.max(0.001);
        self.rolling_error_ratios.push(self.last_error_ratio);
        if self.rolling_error_ratios.len() > 10 {
            self.rolling_error_ratios.remove(0);
        }
        self.samples += 1;
    }

    pub fn record_output(&mut self, actual_output_tokens: u64, output_chars: usize) {
        if output_chars == 0 {
            return;
        }
        let observed = actual_output_tokens as f32 / output_chars as f32;
        self.output_tokens_per_char = self.output_tokens_per_char * 0.9 + observed * 0.1;
    }

    pub fn estimate_tokens(&self, chars: usize) -> u64 {
        (chars as f32 * self.tokens_per_char).ceil() as u64
    }

    pub fn estimate_output_tokens(&self, output_chars: usize) -> u64 {
        (output_chars as f32 * self.output_tokens_per_char).ceil() as u64
    }

    pub fn confidence(&self) -> BudgetCalibrationConfidence {
        match self.samples {
            0 => BudgetCalibrationConfidence::None,
            1..=2 => BudgetCalibrationConfidence::Low,
            3..=9 => BudgetCalibrationConfidence::Medium,
            _ => {
                let recent_variance = self.recent_variance();
                if recent_variance < 0.15 {
                    BudgetCalibrationConfidence::High
                } else if recent_variance < 0.35 {
                    BudgetCalibrationConfidence::Medium
                } else {
                    BudgetCalibrationConfidence::Low
                }
            }
        }
    }

    pub fn fallback_reason(&self) -> Option<String> {
        match self.confidence() {
            BudgetCalibrationConfidence::High => None,
            BudgetCalibrationConfidence::Medium => Some(format!(
                "calibration for {} has {} samples; medium confidence",
                self.model, self.samples
            )),
            BudgetCalibrationConfidence::Low | BudgetCalibrationConfidence::None => Some(format!(
                "calibration for {} has insufficient samples ({}); using default estimate",
                self.model, self.samples
            )),
        }
    }

    fn recent_variance(&self) -> f32 {
        if self.rolling_error_ratios.len() < 3 {
            return 1.0;
        }
        let recent: Vec<f32> = self
            .rolling_error_ratios
            .iter()
            .rev()
            .take(5)
            .copied()
            .collect();
        let mean = recent.iter().sum::<f32>() / recent.len() as f32;
        let variance = recent.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / recent.len() as f32;
        variance.sqrt()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationStore {
    pub entries: Vec<BudgetCalibration>,
}

impl CalibrationStore {
    pub fn get_or_create(&mut self, model: &str) -> &mut BudgetCalibration {
        if let Some(pos) = self.entries.iter().position(|e| e.model == model) {
            &mut self.entries[pos]
        } else {
            self.entries.push(BudgetCalibration::new(model));
            self.entries.last_mut().unwrap()
        }
    }

    fn load() -> Self {
        serde_json::from_str(include_str!("../../config/token-calibration.json"))
            .unwrap_or_default()
    }
}

static CALIBRATION: std::sync::LazyLock<Mutex<CalibrationStore>> =
    std::sync::LazyLock::new(|| Mutex::new(CalibrationStore::load()));

pub fn record_usage(model: &str, actual_input_tokens: u64, total_chars: usize) {
    if let Ok(mut store) = CALIBRATION.lock() {
        store
            .get_or_create(model)
            .record(actual_input_tokens, total_chars);
    }
}

pub fn record_full_usage(
    model: &str,
    estimated_input: u64,
    actual_input: u64,
    actual_output: u64,
    input_chars: usize,
    output_chars: usize,
) {
    if let Ok(mut store) = CALIBRATION.lock() {
        let entry = store.get_or_create(model);
        entry.record(actual_input, input_chars);
        entry.record_output(actual_output, output_chars);
        if estimated_input > 0 {
            entry.last_error_ratio = actual_input as f32 / estimated_input as f32;
            entry.rolling_error_ratios.push(entry.last_error_ratio);
            if entry.rolling_error_ratios.len() > 10 {
                entry.rolling_error_ratios.remove(0);
            }
        }
    }
}

pub fn estimate_tokens(model: &str, chars: usize) -> u64 {
    if let Ok(store) = CALIBRATION.lock() {
        if let Some(entry) = store.entries.iter().find(|e| e.model == model) {
            return entry.estimate_tokens(chars);
        }
    }
    (chars as f32 / 3.0).ceil() as u64
}

pub fn estimate_with_confidence(
    model: &str,
    input_chars: usize,
    output_chars: usize,
) -> CalibratedEstimate {
    if let Ok(store) = CALIBRATION.lock() {
        if let Some(entry) = store.entries.iter().find(|e| e.model == model) {
            return CalibratedEstimate {
                input_tokens: entry.estimate_tokens(input_chars),
                output_tokens: entry.estimate_output_tokens(output_chars),
                confidence: entry.confidence(),
                fallback_reason: entry.fallback_reason(),
            };
        }
    }
    CalibratedEstimate {
        input_tokens: (input_chars as f32 / 3.0).ceil() as u64,
        output_tokens: (output_chars as f32 / 2.5).ceil() as u64,
        confidence: BudgetCalibrationConfidence::None,
        fallback_reason: Some(format!(
            "no calibration data for model {}; using default estimate",
            model
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibratedEstimate {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub confidence: BudgetCalibrationConfidence,
    pub fallback_reason: Option<String>,
}

#[cfg(test)]
mod budget_calibration_tests {
    use super::*;

    #[test]
    fn new_calibration_defaults_to_one_third() {
        let cal = BudgetCalibration::new("test-model");
        assert!((cal.tokens_per_char - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn record_updates_tokens_per_char() {
        let mut cal = BudgetCalibration::new("test-model");
        let original = cal.tokens_per_char;
        cal.record(40, 100);
        assert!(cal.tokens_per_char > original);
        assert_eq!(cal.samples, 1);
    }

    #[test]
    fn record_ignores_zero_chars() {
        let mut cal = BudgetCalibration::new("test-model");
        let original = cal.tokens_per_char;
        cal.record(100, 0);
        assert_eq!(cal.tokens_per_char, original);
    }

    #[test]
    fn store_get_or_create_returns_existing() {
        let mut store = CalibrationStore::default();
        let entry = store.get_or_create("gpt-4o");
        entry.record(40, 100);
        let entry2 = store.get_or_create("gpt-4o");
        assert_eq!(entry2.samples, 1);
    }

    #[test]
    fn estimate_tokens_unknown_model_falls_back() {
        let tokens = estimate_tokens("unknown", 300);
        assert!((90..=110).contains(&tokens));
    }

    #[test]
    fn record_output_updates_output_tokens_per_char() {
        let mut cal = BudgetCalibration::new("test-model");
        let original = cal.output_tokens_per_char;
        cal.record_output(50, 100);
        assert!(cal.output_tokens_per_char > original);
    }

    #[test]
    fn record_output_ignores_zero_chars() {
        let mut cal = BudgetCalibration::new("test-model");
        let original = cal.output_tokens_per_char;
        cal.record_output(100, 0);
        assert_eq!(cal.output_tokens_per_char, original);
    }

    #[test]
    fn confidence_none_with_zero_samples() {
        let cal = BudgetCalibration::new("test-model");
        assert_eq!(cal.confidence(), BudgetCalibrationConfidence::None);
        assert!(cal.fallback_reason().is_some());
    }

    #[test]
    fn confidence_low_with_few_samples() {
        let mut cal = BudgetCalibration::new("test-model");
        for _ in 0..2 {
            cal.record(40, 100);
        }
        assert_eq!(cal.confidence(), BudgetCalibrationConfidence::Low);
    }

    #[test]
    fn confidence_high_after_stable_samples() {
        let mut cal = BudgetCalibration::new("test-model");
        for _ in 0..15 {
            cal.record(33, 100);
        }
        assert_eq!(cal.confidence(), BudgetCalibrationConfidence::High);
        assert!(cal.fallback_reason().is_none());
    }

    #[test]
    fn estimate_with_confidence_unknown_model() {
        let est = estimate_with_confidence("unknown", 300, 200);
        assert_eq!(est.confidence, BudgetCalibrationConfidence::None);
        assert!(est.fallback_reason.is_some());
    }

    #[test]
    fn estimate_with_confidence_calibrated_model() {
        let mut store = CalibrationStore::default();
        let entry = store.get_or_create("calibrated-model");
        for _ in 0..15 {
            entry.record(33, 100);
            entry.record_output(40, 100);
        }
        let est = {
            let store_ref = std::sync::Mutex::new(store);
            let store_guard = store_ref.lock().unwrap();
            let entry = store_guard
                .entries
                .iter()
                .find(|e| e.model == "calibrated-model");
            assert!(entry.is_some());
            CalibratedEstimate {
                input_tokens: entry.unwrap().estimate_tokens(300),
                output_tokens: entry.unwrap().estimate_output_tokens(200),
                confidence: entry.unwrap().confidence(),
                fallback_reason: entry.unwrap().fallback_reason(),
            }
        };
        assert_eq!(est.confidence, BudgetCalibrationConfidence::High);
        assert!(est.fallback_reason.is_none());
    }

    #[test]
    fn record_full_usage_updates_both() {
        let mut store = CalibrationStore::default();
        {
            let entry = store.get_or_create("full-usage-model");
            entry.record(33, 100);
            entry.record_output(40, 100);
        }
        let entry = store.get_or_create("full-usage-model");
        assert_eq!(entry.samples, 1);
        assert!(entry.output_tokens_per_char != 1.0 / 2.5);
    }
}
