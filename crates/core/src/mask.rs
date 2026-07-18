//! Bidirectional mask engine.
//!
//! One grammar, two directions — this is an invariant, not a preference
//! (architecture.md). The same pattern string, e.g. `%artist% - %title%`,
//! must both *render* a filename from tags and *extract* tags from a
//! filename. A single implementation for both directions is mandatory:
//! divergent placeholder behavior between rename and import is the worst
//! class of bug this tool can have.

use crate::model::TagMap;
use thiserror::Error;

/// A parsed, validated mask pattern.
#[derive(Debug, Clone)]
pub struct Mask {
    pattern: String,
}

impl Mask {
    /// Parse and validate a pattern string.
    pub fn parse(pattern: &str) -> Result<Self, MaskError> {
        // TODO: tokenize into literal/placeholder segments, validate
        // placeholder names against TagField, reject adjacent placeholders
        // without a literal separator (ambiguous for extraction).
        Ok(Self {
            pattern: pattern.to_string(),
        })
    }

    /// Tags -> filename (the Music Renamer direction).
    pub fn render(&self, _tags: &TagMap) -> Result<String, MaskError> {
        todo!("substitute placeholders from tags; error on missing fields")
    }

    /// Filename -> tags (the import direction).
    pub fn extract(&self, _filename: &str) -> Result<TagMap, MaskError> {
        todo!("match literal segments, capture placeholder spans into TagMap")
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

#[derive(Debug, Error)]
pub enum MaskError {
    #[error("unknown placeholder: %{0}%")]
    UnknownPlaceholder(String),
    #[error("ambiguous pattern: adjacent placeholders without a separator")]
    Ambiguous,
    #[error("missing tag for placeholder: %{0}%")]
    MissingTag(String),
    #[error("pattern does not match the filename")]
    NoMatch,
}
