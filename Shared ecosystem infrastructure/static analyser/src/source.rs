/// A source unit supplied to the analyser.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

impl SourceFile {
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
        }
    }

    pub fn line_col(&self, byte_offset: usize) -> (usize, usize) {
        let offset = byte_offset.min(self.text.len());
        let prefix = &self.text[..self.text.floor_char_boundary(offset)];
        let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        (line, column)
    }
}
