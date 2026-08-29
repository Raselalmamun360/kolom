pub mod emit;
pub mod link;
pub mod spike;

pub use emit::{emit, emit_for};
pub use link::{link_executable, link_executable_for, LinkError, Target};
pub use spike::build_hello_object;
