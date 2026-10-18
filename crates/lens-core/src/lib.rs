//! Agent Session Lens core. See docs/architecture-v3.md.

pub mod aggregator;
pub mod agy;
pub mod change_detector;
pub mod claude;
pub mod clock;
pub mod clock_offset;
pub mod codex;
pub mod consts;
pub mod distro;
pub mod engine;
pub mod ipc;
pub mod model;
pub mod ndjson;
pub mod process;
pub mod project;
pub mod reader;
pub mod redact;
pub mod runtime;
pub mod session;
pub mod supervisor;
pub mod settings;
mod util;

pub use engine::{default_sources, Engine, SourceConfig};
pub use model::Snapshot;
