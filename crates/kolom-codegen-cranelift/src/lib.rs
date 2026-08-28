pub mod emit;
pub mod link;
pub mod spike;

pub use emit::emit;
pub use link::{link_executable, LinkError};
pub use spike::build_hello_object;
