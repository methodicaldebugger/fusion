// contents of span.rs


#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn point(position: usize) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn contains(self, position: usize) -> bool {
        self.start <= position && position < self.end
    }

    pub fn merge(self, other: Span) -> Self {
    let start = if self.start < other.start {
        self.start
    } else {
        other.start
    };

    let end = if self.end > other.end {
        self.end
    } else {
        other.end
    };

    Self { start, end }
}

    pub fn join(self, other: Span) -> Self {
        self.merge(other)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, start: usize, end: usize) -> Self {
        Self {
            node,
            span: Span::new(start, end),
        }
    }

    pub fn from_span(node: T, span: Span) -> Self {
        Self { node, span }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned {
            node: f(self.node),
            span: self.span,
        }
    }
}