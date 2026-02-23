pub mod document;
pub mod element;

pub use document::Document;
pub use element::{NodeFlags, NodeType, PawsElement};

// Re-export PawsElement as Node to satisfy older uses if any, or just use PawsElement directly.
pub use element::PawsElement as Node;
