//! Implements the contract's document protocol for eval-owned documents.

/// `Document` (and optionally `Identified`) for a type with a `schema`
/// field, as the contract's own documents do.
macro_rules! eval_document {
    ($ty:ty, $schema:expr, $limits:expr) => {
        impl rustev_contract::Document for $ty {
            const SCHEMA: &'static str = $schema;
            const LIMITS: rustev_contract::bounded::ParseLimits = $limits;
            fn schema(&self) -> &str {
                &self.schema
            }
        }
    };
    ($ty:ty, $schema:expr, $limits:expr, $id:ty) => {
        $crate::doc::eval_document!($ty, $schema, $limits);
        impl rustev_contract::Identified for $ty {
            type Id = $id;
            fn wrap(digest: String) -> $id {
                <$id>::parse(&digest).expect("a tagged digest is an identity")
            }
        }
    };
}

pub(crate) use eval_document;
