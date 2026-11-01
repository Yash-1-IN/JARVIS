//! Model price table (D-16). Prices live in `resources/pricing.json`, embedded in the
//! binary and not user-editable. A model without a price is rejected rather than guessed,
//! so the spend cap can never silently under-count.

use super::spend::{Micros, Price};
use serde::Deserialize;
use std::collections::BTreeMap;

const PRICING_JSON: &str = include_str!("../../resources/pricing.json");

/// Cheapest, fastest current tier, chosen for voice latency (spec §7.4). Must be in the table.
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    input_per_mtok: Micros,
    output_per_mtok: Micros,
}

#[derive(Debug, Deserialize)]
struct File {
    version: u32,
    models: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct PriceTable {
    models: BTreeMap<String, Price>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PricingError {
    #[error("pricing data is malformed: {0}")]
    Malformed(String),
    #[error("unsupported pricing version {0}")]
    Version(u32),
    #[error("model `{0}` has no price")]
    UnknownModel(String),
}

impl PriceTable {
    /// The table embedded in the app.
    pub fn embedded() -> Result<Self, PricingError> {
        Self::parse(PRICING_JSON)
    }

    pub fn parse(json: &str) -> Result<Self, PricingError> {
        let f: File =
            serde_json::from_str(json).map_err(|e| PricingError::Malformed(e.to_string()))?;
        if f.version != 1 {
            return Err(PricingError::Version(f.version));
        }
        let models = f
            .models
            .into_iter()
            .map(|(id, e)| {
                (
                    id,
                    Price {
                        input_per_mtok: e.input_per_mtok,
                        output_per_mtok: e.output_per_mtok,
                    },
                )
            })
            .collect();
        Ok(Self { models })
    }

    pub fn price(&self, model: &str) -> Result<Price, PricingError> {
        self.models
            .get(model)
            .copied()
            .ok_or_else(|| PricingError::UnknownModel(model.to_owned()))
    }

    /// Priced model ids, sorted: the `availableModels` of the settings contract.
    pub fn model_ids(&self) -> Vec<String> {
        self.models.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_parses_and_prices_the_default_model() {
        let t = PriceTable::embedded().unwrap();
        let p = t.price(DEFAULT_MODEL).unwrap();
        assert!(p.input_per_mtok > 0 && p.output_per_mtok > p.input_per_mtok);
    }

    #[test]
    fn every_listed_model_has_nonzero_prices_and_ids_are_sorted() {
        let t = PriceTable::embedded().unwrap();
        let ids = t.model_ids();
        assert!(ids.len() >= 2);
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        for id in &ids {
            let p = t.price(id).unwrap();
            assert!(p.input_per_mtok > 0 && p.output_per_mtok > 0, "{id}");
        }
    }

    #[test]
    fn unknown_model_is_rejected_not_guessed() {
        let t = PriceTable::embedded().unwrap();
        assert_eq!(
            t.price("claude-made-up"),
            Err(PricingError::UnknownModel("claude-made-up".into()))
        );
    }

    #[test]
    fn bad_data_is_an_error() {
        assert!(matches!(
            PriceTable::parse("{"),
            Err(PricingError::Malformed(_))
        ));
        assert_eq!(
            PriceTable::parse(r#"{"version":2,"models":{}}"#).unwrap_err(),
            PricingError::Version(2)
        );
        assert!(matches!(
            PriceTable::parse(r#"{"version":1,"models":{"m":{"inputPerMtok":1}}}"#),
            Err(PricingError::Malformed(_))
        ));
    }

    #[test]
    fn a_short_haiku_turn_costs_a_fraction_of_a_cent() {
        use crate::intelligence::provider::Usage;
        let p = PriceTable::embedded()
            .unwrap()
            .price("claude-haiku-5-5")
            .unwrap();
        // 2,000 input + 300 output tokens at $0.10 / $0.50 per million.
        assert_eq!(
            p.cost(Usage {
                input_tokens: 2_000,
                output_tokens: 300
            }),
            200 + 150
        );
    }
}
