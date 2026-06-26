//! XPath 2.0 parsing and evaluation.
//!
//! This module is implemented beside the existing XPath 1.0 engine so the
//! legacy API remains unchanged while XPath 2.0 grows toward full conformance.

pub mod ast;
pub mod evaluator;
pub mod functions;
pub mod lexer;
pub mod parser;
pub mod types;
pub mod value;

pub use evaluator::{NoopXPath2Resolver, XPath2Evaluator, XPath2Options, XPath2Resolver};
pub use types::AtomicType;
pub use value::{QNameValue, XPath2AtomicValue, XPath2Item, XPath2Value};
