mod engine_matcher;
mod engine_registry;
mod engine_runtime;
mod workspace;

pub use engine_matcher::{engine_match_specificity, engine_matches, media_type_ancestors};
pub use engine_registry::{
    EngineCandidate, EnginePreferences, EngineResolution, EngineResolutionSource, builtin_engines,
    list_matching_engines, resolve_default_engine,
};
pub use engine_runtime::{
    EnginePlatform, EngineRuntimeCapability, EngineRuntimeKind, engine_runtime_capabilities,
};
pub use workspace::{ExplorerLayoutState, TabDescriptor, WorkspaceKind, WorkspaceState, tab_key};
