use crate::common::*;

async fn jobs_request(
    base_url: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> (reqwest::StatusCode, serde_json::Value) {
    let session = console_login(base_url).await;
    let mut request = client()
        .request(method, format!("{base_url}{path}"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", base_url);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap_or_default())
}

#[tokio::test]
async fn test_housekeeping_job_runs_and_is_listed() {
    let base_url = start_server().await;

    let (status, started) = jobs_request(
        &base_url,
        reqwest::Method::POST,
        "/api/jobs",
        Some(serde_json::json!({"kind": "housekeeping"})),
    )
    .await;
    assert_eq!(status, 200, "{started}");
    assert_eq!(started["job"]["kind"], "housekeeping");
    assert_eq!(started["job"]["trigger"], "manual");

    let id = started["job"]["id"].as_str().unwrap();
    for _ in 0..100 {
        let (_, body) = jobs_request(
            &base_url,
            reqwest::Method::GET,
            &format!("/api/jobs/{id}"),
            None,
        )
        .await;
        if body["job"]["status"] != "running" {
            assert_eq!(body["job"]["status"], "completed", "{body}");
            assert_eq!(body["job"]["detail"]["uploadsRemoved"], 0, "{body}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let (status, listed) = jobs_request(
        &base_url,
        reqwest::Method::GET,
        "/api/jobs?kind=housekeeping",
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(listed["jobs"][0]["id"], id);

    let (status, _) = jobs_request(
        &base_url,
        reqwest::Method::DELETE,
        &format!("/api/jobs/{id}"),
        None,
    )
    .await;
    assert_eq!(status, 404, "a finished job cannot be cancelled");
}

#[tokio::test]
async fn test_job_scope_is_validated() {
    let base_url = start_server().await;

    let (status, _) = jobs_request(
        &base_url,
        reqwest::Method::POST,
        "/api/jobs",
        Some(serde_json::json!({"kind": "housekeeping", "scope": "some-bucket"})),
    )
    .await;
    assert_eq!(status, 409);

    let (status, _) = jobs_request(
        &base_url,
        reqwest::Method::POST,
        "/api/jobs",
        Some(serde_json::json!({"kind": "compressBucket", "scope": "missing-bucket"})),
    )
    .await;
    assert_eq!(status, 404);
}
