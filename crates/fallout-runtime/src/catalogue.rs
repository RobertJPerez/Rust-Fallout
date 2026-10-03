//! Source ownership is separate from mutable campaign state. A shared catalogue
//! lets a world outlive its loader while several worlds reuse the same records.
use fallout_data::loaded_scripts::Catalogue;
use std::{ops::Deref, sync::Arc};

#[derive(Clone)]
pub enum SourceCatalogue<'a> {
    Borrowed(&'a Catalogue),
    Shared(Arc<Catalogue>),
}

impl Deref for SourceCatalogue<'_> {
    type Target = Catalogue;

    fn deref(&self) -> &Catalogue {
        match self {
            Self::Borrowed(catalogue) => catalogue,
            Self::Shared(catalogue) => catalogue,
        }
    }
}

impl<'a> From<&'a Catalogue> for SourceCatalogue<'a> {
    fn from(catalogue: &'a Catalogue) -> Self {
        Self::Borrowed(catalogue)
    }
}

impl From<Arc<Catalogue>> for SourceCatalogue<'_> {
    fn from(catalogue: Arc<Catalogue>) -> Self {
        Self::Shared(catalogue)
    }
}
