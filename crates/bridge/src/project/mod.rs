//! Extended project.json parser.

pub mod condition;
pub mod manifest;
pub mod overrides;
pub mod property;

pub use condition::{CmpOp, Condition, Literal};
pub use manifest::{ProjectModel, ProjectProperty};
pub use overrides::OverrideMapExt;
pub use property::{ComboOption, PropertyKind, PropertyMetadata, PropertyValue};
