//! Natural language command parsing and execution.
//!
//! This module provides AI-powered natural language understanding:
//! - Intent parsing (convert text to structured commands)
//! - Intent execution (run parsed commands)
//!
//! # Example
//!
//! ```ignore
//! use dtx_protocol::nl::{IntentParser, ParsedIntent};
//!
//! let parser = IntentParser::new(ai_provider);
//! let intent = parser.parse("start the database").await?;
//! // intent.operation == "start", intent.targets == ["database"]
//! ```

mod executor;
mod parser;

pub use executor::IntentExecutor;
pub use parser::{IntentParser, ParsedIntent};
