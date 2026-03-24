//! Context assembly, compression, and token budget management.
//!
//! This crate provides the machinery to build a context window for LLM calls:
//! estimate token counts, deduplicate items, compress when needed, and assemble
//! everything into a budget-respecting [`AssembledContext`].

pub mod assembler;
pub mod builder;
pub mod compressor;
pub mod dedup;
pub mod estimator;
pub mod step_scope;

pub use assembler::{CategoryBudgets, ContextAssembler};
pub use builder::ContextBuilder;
pub use compressor::{ContextCompressor, SummaryCompressor, TruncationCompressor};
pub use dedup::ContextDeduplicator;
pub use estimator::TokenEstimator;
pub use step_scope::StepContextBuilder;
