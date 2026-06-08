mod checksum;
mod conditions;
mod copy;
mod delete;
mod get;
mod post;
mod put;
mod tagging;

#[cfg(test)]
mod tests;

pub(crate) use checksum::{body_to_reader, extract_checksum};

pub use delete::{delete_object, delete_objects};
pub use get::{get_object, head_object};
pub use post::post_object;
pub use put::put_object;
