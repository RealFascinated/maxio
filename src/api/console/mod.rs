mod access;
mod admin;
mod auth;
mod bucket_settings;
mod buckets;
mod maintenance;
mod metrics;
mod objects;
mod session;
mod versions;

#[cfg(test)]
mod tests;

pub use session::LoginRateLimiter;

use auth::{
    auth_config, check, console_auth_middleware, console_csrf_middleware, login, logout,
    require_root_middleware,
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};
use bucket_settings::{get_cors, get_public, get_versioning, set_cors, set_public, set_versioning};
use buckets::{create_bucket, delete_bucket_api, list_buckets};
use maintenance::{repair_orphan_meta_api, scan_orphan_meta_api};
use metrics::get_metrics_api;
use objects::{
    create_folder, delete_folder, delete_object_api, download_object, get_object_api, list_objects,
    preview_folder_delete, presign_object, upload_object,
};
use versions::{delete_version, download_version, list_versions};

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
        .route(
            "/buckets/{bucket}/objects/{*key}",
            get(get_object_api).delete(delete_object_api),
        )
        .route("/buckets/{bucket}/download/{*key}", get(download_object))
        .route("/buckets/{bucket}/presign/{*key}", get(presign_object))
        .route("/buckets/{bucket}/versioning", get(get_versioning))
        .route("/buckets/{bucket}/versioning", put(set_versioning))
        .route("/buckets/{bucket}/public", get(get_public))
        .route("/buckets/{bucket}/public", put(set_public))
        .route("/buckets/{bucket}/cors", get(get_cors))
        .route("/buckets/{bucket}/cors", put(set_cors))
        .route("/buckets/{bucket}/versions", get(list_versions))
        .route(
            "/buckets/{bucket}/versions/{version_id}/objects/{*key}",
            delete(delete_version),
        )
        .route(
            "/buckets/{bucket}/versions/{version_id}/download/{*key}",
            get(download_version),
        )
        .merge(admin_routes)
        .layer(json_body_limit);

    let protected_streaming =
        Router::new().route("/buckets/{bucket}/upload/{*key}", put(upload_object));

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
