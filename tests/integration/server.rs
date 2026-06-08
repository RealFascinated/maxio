use crate::common::*;

#[tokio::test]
async fn test_healthz_is_public_and_returns_ok() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/healthz", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_auth_config_is_public() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/api/auth/config", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["cookiesRequireHttps"], false);
}

#[tokio::test]
async fn test_readyz_is_public_and_returns_ok() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/readyz", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_security_headers_are_applied() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/healthz", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert!(resp.headers().contains_key("content-security-policy"));
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
}

#[tokio::test]
async fn test_ui_deep_link_uses_spa_fallback() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/ui/buckets/example/settings", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/html")
    );
    assert_eq!(
        resp.headers().get("cache-control").unwrap(),
        "no-store, must-revalidate"
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("MaxIO"));
}
