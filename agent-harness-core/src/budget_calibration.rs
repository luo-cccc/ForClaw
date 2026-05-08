use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetCalibration {
    pub model: String,
    pub tokens_per_char: f32,
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

    pub fn estimate_tokens(&self, chars: usize) -> u64 {
        (chars as f32 * self.tokens_per_char).ceil() as u64
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

pub fn estimate_tokens(model: &str, chars: usize) -> u64 {
    if let Ok(store) = CALIBRATION.lock() {
        if let Some(entry) = store.entries.iter().find(|e| e.model == model) {
            return entry.estimate_tokens(chars);
        }
    }
    (chars as f32 / 3.0).ceil() as u64
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
}
