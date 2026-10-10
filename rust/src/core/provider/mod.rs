//! Provider trait - defines the interface all providers must implement

mod context;
mod error;
mod id;
mod spec;
mod traits;

pub use context::*;
pub use error::*;
pub use id::*;
pub use spec::ProviderMetadata;
pub use traits::*;
