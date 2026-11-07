//! The enocone flow document format.
//!
//! A flow is a Markdown document people can read (title, why, Given/When/Then
//! sentences) whose fenced `http` / `browser` / `expect` / `use` blocks make
//! it executable. The same document, plus run results, is the report.

pub mod blocks;
pub mod design;
pub mod model;
pub mod parse;
pub mod results;
pub mod template;

pub use model::*;
pub use parse::{Diagnostic, Parsed, Severity, parse_flow, parse_yaml};
pub use results::*;
