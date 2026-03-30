pub mod engine;
pub mod registry;
pub mod rule_selector;

mod architecture;
mod configuration;
mod correctness;
mod performance;
mod pydantic;
mod resilience;
mod routes;
mod security;

#[cfg(test)]
mod tests;

pub use engine::{
    analyze_module, analyze_module_with_suite, analyze_project_modules, analyze_routes,
    route_checks_not_evaluated, RuleSelection,
};
pub use registry::StaticRule;
pub use rule_selector::select_rule_ids;
