mod access;
mod admin;
mod admin_views;
mod auth;
mod bucket_settings;
mod buckets;
mod error;
mod maintenance;
mod metrics;
mod objects;
mod service;
mod session;
mod types;
mod versions;

pub use session::{LoginRateLimiter, RevokedSessions};

#[allow(unused_imports)]
#[doc(hidden)]
pub use types::{ObjectGetOp, ObjectGetQuery, sanitize_filename};

#[allow(unused_imports)]
#[doc(hidden)]
pub use service::{
    folder_delete_stats, normalize_folder_prefix, normalize_presign_host,
    parent_folder_prefix_for_deleted_object, preserve_empty_parent_folder_after_object_delete,
};

use auth::{
    auth_config, check, console_auth_middleware, console_csrf_middleware, login, logout,
    require_root_middleware,
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};
use bucket_settings::{
    get_cors, get_lifecycle, get_public, get_versioning, set_cors, set_lifecycle, set_public,
    set_versioning,
};
use buckets::{create_bucket, delete_bucket_api, list_buckets};
use maintenance::{repair_orphan_meta_api, scan_orphan_meta_api};
use metrics::get_metrics_api;
use objects::{
    create_folder, delete_folder, delete_object_handler, delete_objects_api, get_object_handler,
    list_objects, preview_folder_delete, upload_object,
};
use versions::list_versions;

use crate::server::AppState;

pub fn console_router(state: AppState) -> Router<AppState> {
    let json_body_limit = DefaultBodyLimit::max(state.config.max_console_body_bytes);

    let public = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/check", get(check))
        .route("/auth/config", get(auth_config))
        .layer(json_body_limit);

    let admin_routes: Router<AppState> = Router::new()
        .route("/maintenance/orphan-meta", get(scan_orphan_meta_api))
        .route(
            "/maintenance/orphan-meta/repair",
            post(repair_orphan_meta_api),
        )
        .route("/metrics", get(get_metrics_api))
        .route("/users", get(admin::list_users_api))
        .route("/users", post(admin::create_user_api))
        .route("/users/{username}", delete(admin::delete_user_api))
        .route("/users/{username}/keys", post(admin::create_user_key_api))
        .route(
            "/users/{username}/keys/{access_key_id}",
            delete(admin::delete_user_key_api),
        )
        .route(
            "/users/{username}/policies/{policy_name}",
            get(admin::get_user_policy_api)
                .put(admin::put_user_policy_api)
                .delete(admin::delete_user_policy_api),
        )
        .route(
            "/users/{username}/attach-policy",
            post(admin::attach_user_policy_api),
        )
        .route(
            "/users/{username}/detach-policy",
            post(admin::detach_user_policy_api),
        )
        .route(
            "/policies",
            get(admin::list_policies_api).post(admin::create_policy_api),
        )
        .route(
            "/policies/{name}",
            get(admin::get_policy_api).delete(admin::delete_policy_api),
        )
        .layer(axum::middleware::from_fn(require_root_middleware));

    let protected_limited = Router::new()
        .route("/auth/logout", post(logout))
        .route("/buckets", get(list_buckets))
        .route("/buckets", post(create_bucket))
        .route("/buckets/{bucket}", delete(delete_bucket_api))
        .route(
            "/buckets/{bucket}/folders",
            post(create_folder).delete(delete_folder),
        )
        .route(
            "/buckets/{bucket}/folders/preview",
            post(preview_folder_delete),
        )
        .route("/buckets/{bucket}/objects", get(list_objects))
        .route("/buckets/{bucket}/objects/delete", post(delete_objects_api))
        .route(
            "/buckets/{bucket}/objects/{*key}",
            get(get_object_handler).delete(delete_object_handler),
        )
        .route("/buckets/{bucket}/versioning", get(get_versioning))
        .route("/buckets/{bucket}/versioning", put(set_versioning))
        .route("/buckets/{bucket}/public", get(get_public))
        .route("/buckets/{bucket}/public", put(set_public))
        .route("/buckets/{bucket}/cors", get(get_cors))
        .route("/buckets/{bucket}/cors", put(set_cors))
        .route("/buckets/{bucket}/lifecycle", get(get_lifecycle))
        .route("/buckets/{bucket}/lifecycle", put(set_lifecycle))
        .route("/buckets/{bucket}/versions", get(list_versions))
        .merge(admin_routes)
        .layer(json_body_limit);

    let protected_streaming =
        Router::new().route("/buckets/{bucket}/objects/{*key}", put(upload_object));

    let protected = protected_limited
        .merge(protected_streaming)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            console_csrf_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state,
            console_auth_middleware,
        ));

    public.merge(protected)
}
