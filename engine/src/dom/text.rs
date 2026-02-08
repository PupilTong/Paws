#[derive(Debug, Clone)]
pub struct TextNodeData {
    pub content: String,
}

impl TextNodeData {
    pub fn new(content: String) -> Self {
        Self { content }
    }
}
