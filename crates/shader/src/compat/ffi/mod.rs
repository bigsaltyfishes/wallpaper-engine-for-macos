//! C compatibility boundary for the Rust shader pipeline.

mod abi;
mod callbacks;
mod diagnostics_json;
mod handles;
mod request_json;
mod response_json;

pub use abi::*;
pub use handles::RsShaderProgram;

#[cfg(test)]
mod tests;
