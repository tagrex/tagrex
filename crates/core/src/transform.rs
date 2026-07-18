//! Composable text transforms.
//!
//! Used by masks, manual edits and provider post-processing. Deliberately a
//! chain of steps: Mp3tag-style "actions"/scripting later becomes
//! *serialization of chains into saved presets*, not a new subsystem
//! (architecture.md, "Deferred").

/// A single text transformation over a field value.
pub trait TransformStep: Send + Sync {
    /// Stable identifier for presets and UI.
    fn name(&self) -> &str;
    fn apply(&self, input: &str) -> String;
}

/// An ordered chain of transform steps.
#[derive(Default)]
pub struct TransformChain {
    steps: Vec<Box<dyn TransformStep>>,
}

impl TransformChain {
    pub fn push(&mut self, step: Box<dyn TransformStep>) {
        self.steps.push(step);
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Apply all steps in order.
    pub fn apply(&self, input: &str) -> String {
        self.steps
            .iter()
            .fold(input.to_string(), |acc, step| step.apply(&acc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Uppercase;

    impl TransformStep for Uppercase {
        fn name(&self) -> &str {
            "uppercase"
        }
        fn apply(&self, input: &str) -> String {
            input.to_uppercase()
        }
    }

    #[test]
    fn chain_applies_steps_in_order() {
        let mut chain = TransformChain::default();
        chain.push(Box::new(Uppercase));
        assert_eq!(chain.apply("tagrex"), "TAGREX");
    }

    #[test]
    fn empty_chain_is_identity() {
        let chain = TransformChain::default();
        assert_eq!(chain.apply("tagrex"), "tagrex");
    }
}
