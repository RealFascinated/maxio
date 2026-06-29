use axum::{
    Router,
    body::Body,
    response::Response,
    routing::{delete, get, head, options, post, put},
};
use http::StatusCode;

use crate::server::AppState;

use super::{bucket, list, object};

/// Dummy OPTIONS handler — the real preflight logic runs in the CORS middleware.
async fn options_handler() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap()
}

macro_rules! bucket_route {
    ($router:expr, $method:ident, $handler:expr) => {
        $router
            .route("/{bucket}", $method($handler))
            .route("/{bucket}/", $method($handler))
    };
}

pub fn s3_router() -> Router<AppState> {
    let router = Router::new().route("/", get(bucket::list_buckets));
    let router = bucket_route!(router, put, bucket::handle_bucket_put);
    let router = bucket_route!(router, head, bucket::head_bucket);
    let router = bucket_route!(router, delete, bucket::delete_bucket);
    let router = bucket_route!(router, get, list::handle_bucket_get);
    let router = bucket_route!(router, options, options_handler);
    let router = bucket_route!(router, post, object::delete_objects);
    router
        .route("/{bucket}/{*key}", post(object::post_object))
        .route("/{bucket}/{*key}", put(object::put_object))
        .route("/{bucket}/{*key}", get(object::get_object))
        .route("/{bucket}/{*key}", head(object::head_object))
        .route("/{bucket}/{*key}", delete(object::delete_object))
        .route("/{bucket}/{*key}", options(options_handler))
}
