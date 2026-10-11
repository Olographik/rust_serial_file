#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct SearchIndexes {
    /// The element before key
    pub before: Option<usize>,
    /// The element matching key
    pub exact: Option<usize>,
    /// The element after key
    pub after: Option<usize>,
}

impl SearchIndexes {
    pub fn none() -> Self {
        Self {
            before: None,
            exact: None,
            after: None,
        }
    }

    pub fn from_exact_point(exact: usize, size: usize) -> Self {
        Self {
            before: if exact > 0 { Some(exact - 1) } else { None },
            exact: Some(exact),
            after: if exact + 1 < size {
                Some(exact + 1)
            } else {
                None
            },
        }
    }

    pub fn from_after_point(after: usize) -> Self {
        Self {
            before: if after > 0 { Some(after - 1) } else { None },
            exact: None,
            after: Some(after),
        }
    }
    pub fn from_before_point(before: usize, size: usize) -> Self {
        Self {
            before: Some(before),
            exact: None,
            after: if before + 1 < size {
                Some(before + 1)
            } else {
                None
            },
        }
    }
}

pub struct SearchContext(pub Option<usize>);

impl SearchContext {
    pub fn none() -> Self {
        Self(None)
    }

    pub fn of(index: usize) -> Self {
        Self(Some(index))
    }

    pub fn update_context(&mut self, result: &SearchIndexes) {
        if let Some(exact) = result.exact {
            self.0 = Some(exact);
        } else if let Some(before) = result.before {
            self.0 = Some(before)
        } else if let Some(after) = result.after {
            self.0 = Some(after)
        }
    }
}

impl Into<SearchContext> for usize {
    fn into(self) -> SearchContext {
        SearchContext::of(self)
    }
}
