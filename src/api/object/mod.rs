mod conditions;
mod copy;
mod delete;
mod get;
mod post;
mod put;
mod tagging;

#[allow(unused_imports)]
pub use conditions::{ConditionalResult, check_conditions, etag_matches};
#[allow(unused_imports)]
pub use delete::parse_delete_objects_xml;
pub(crate) use delete::{delete_object, delete_objects};
pub use get::{get_object, head_object};
pub use post::post_object;
pub use put::put_object;
