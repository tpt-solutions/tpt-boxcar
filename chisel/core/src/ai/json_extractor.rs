use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

pub struct JsonExtractor;

impl JsonExtractor {
    pub fn extract_json<T: DeserializeOwned>(content: &str) -> Result<T> {
        if let Ok(parsed) = serde_json::from_str::<T>(content) {
            return Ok(parsed);
        }

        if let Some(start) = content.find("```json") {
            let json_start = start + 7;
            if let Some(end) = content[json_start..].find("```") {
                let json_str = &content[json_start..json_start + end];
                return serde_json::from_str::<T>(json_str.trim())
                    .context("failed to parse JSON from code block");
            }
        }

        if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                let json_str = &content[start..=end];
                return serde_json::from_str::<T>(json_str)
                    .context("failed to parse JSON from extracted content");
            }
        }

        anyhow::bail!("no valid JSON found in response")
    }
}
