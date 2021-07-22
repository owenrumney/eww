#[derive(Eq, PartialEq, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Span(pub usize, pub usize, pub usize);
pub static DUMMY_SPAN: Span = Span(usize::MAX, usize::MAX, usize::MAX);

impl Span {
    /// Get the span that includes this and the other span completely.
    /// Will panic if the spans are from different file_ids.
    pub fn to(mut self, other: Span) -> Self {
        assert!(other.2 == self.2);
        self.1 = other.1;
        self
    }

    pub fn ending_at(mut self, end: usize) -> Self {
        self.1 = end;
        self
    }

    /// Turn this span into a span only highlighting the point it starts at, setting the length to 0.
    pub fn point_span(mut self) -> Self {
        self.1 = self.0;
        self
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.0, self.1)
    }
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.0, self.1)
    }
}
