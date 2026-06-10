use crate::common::*;

#[tokio::test]
async fn test_console_admin_create_and_list_users() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let create = client()
        .post(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({"username": "console-user"}))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let list = client()
        .get(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    let body: serde_json::Value = list.json().await.unwrap();
    let names: Vec<String> = body["users"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|u| u.get("username").and_then(|v| v.as_str()))
        .map(String::from)
        .collect();
    assert!(names.iter().any(|n| n == "console-user"));
}

#[tokio::test]
async fn test_console_auth_check_and_logout() {
    let base_url = start_server().await;

    let check_before = client()
        .get(format!("{}/api/auth/check", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(check_before.status(), 401);

    let session = console_login(&base_url).await;

    let check = client()
        .get(format!("{}/api/auth/check", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(check.status(), 200);

    let logout = client()
        .post(format!("{}/api/auth/logout", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 200);

    let check_after = client()
        .get(format!("{}/api/auth/check", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(check_after.status(), 401);
}

#[tokio::test]
async fn test_console_metrics_api() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    assert_eq!(
        s3_request("PUT", &format!("{}/metrics-bucket", base_url), vec![])
            .await
            .status(),
        200
    );

    let resp = client()
        .get(format!("{}/api/metrics", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("storage_totals").is_some() || body.get("storage").is_some());
}
