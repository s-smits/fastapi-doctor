pub mod engine;
pub mod registry;
pub mod rule_selector;

mod architecture;
mod configuration;
mod correctness;
mod performance;
mod pydantic;
mod resilience;
mod security;

pub use engine::{RuleSelection, analyze_module, analyze_module_with_suite, analyze_project_modules};
pub use registry::StaticRule;
