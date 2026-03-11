//! Domain types that make illegal states unrepresentable.
//!
//! These types follow the "Parse Don't Validate" pattern:
//! if you have one of these types, it IS valid.
//!
//! # Example
//!
//! ```rust
//! use dtx_core::domain::ServiceName;
//!
//! // Parse from user input
//! let name: ServiceName = "my-api".parse().unwrap();
//!
//! // If we reach here, name IS valid
//! // No further validation needed anywhere in codebase
//! println!("Service: {}", name.as_str());
//! ```

mod environment;
mod port;
mod service_name;
mod shell_command;

pub use environment::{EnvVar, Environment, ParseEnvironmentError};
pub use port::{ParsePortError, Port, MIN_NON_PRIVILEGED_PORT};
pub use service_name::{ParseServiceNameError, ServiceName};
pub use shell_command::{ParseShellCommandError, ShellCommand};
