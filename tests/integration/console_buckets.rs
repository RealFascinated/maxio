use crate::common::*;

#[tokio::test]
async fn test_console_list_buckets_with_stats() {
    let base_url = start_server().await;
    let bucket = "list-stats-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/file.txt", base_url),
            b"hello stats".to_vec(),
        )
        .await
        .status(),
        200
    );

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let entry = body["buckets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == bucket)
        .expect("bucket should appear in list");
    assert!(entry["createdAt"].is_string());
    assert!(entry.get("objectCount").is_some());
    assert!(entry.get("sizeBytes").is_some());
    assert_eq!(entry["canDelete"], true);
    assert_eq!(entry["canManageSettings"], true);
}

#[tokio::test]
async fn test_console_delete_bucket() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-del-bucket";

    let create = client()
        .post(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "name": bucket }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let delete = client()
        .delete(format!("{}/api/buckets/{bucket}", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);

    let head = s3_request("HEAD", &format!("{}/{bucket}", base_url), vec![]).await;
    assert_eq!(head.status(), 404);
}

#[tokio::test]
async fn test_console_create_folder() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "folder-create-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let resp = client()
        .post(format!("{}/api/buckets/{bucket}/folders", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "name": "photos" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let list = client()
        .get(format!("{}/api/buckets/{bucket}/objects?prefix=", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    let body: serde_json::Value = list.json().await.unwrap();
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert_eq!(prefixes, vec!["photos/".to_string()]);
}

#[tokio::test]
async fn test_console_bucket_cors_settings() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-cors-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let get = client()
        .get(format!("{}/api/buckets/{bucket}/cors", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let get_body: serde_json::Value = get.json().await.unwrap();
    assert_eq!(get_body["enabled"], false);

    let set = client()
        .put(format!("{}/api/buckets/{bucket}/cors", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "enabled": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(set.status(), 200);

    let get = client()
        .get(format!("{}/api/buckets/{bucket}/cors", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let get_body: serde_json::Value = get.json().await.unwrap();
    assert_eq!(get_body["enabled"], true);

    let set = client()
        .put(format!("{}/api/buckets/{bucket}/cors", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "enabled": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(set.status(), 200);

    let get = client()
        .get(format!("{}/api/buckets/{bucket}/cors", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let get_body: serde_json::Value = get.json().await.unwrap();
    assert_eq!(get_body["enabled"], false);
}

#[tokio::test]
async fn test_console_versions_list_download_delete() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let bucket = "console-ver-bucket";
    let key = "versioned.txt";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );

    let set_ver = client()
        .put(format!("{}/api/buckets/{bucket}/versioning", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "enabled": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(set_ver.status(), 200);

    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/{key}", base_url),
            b"version-one".to_vec(),
        )
        .await
        .status(),
        200
    );
    let put1 = s3_request(
        "PUT",
        &format!("{}/{bucket}/{key}", base_url),
        b"version-two".to_vec(),
    )
    .await;
    assert_eq!(put1.status(), 200);
    let current_version_id = put1
        .headers()
        .get("x-amz-version-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let list = client()
        .get(format!(
            "{}/api/buckets/{bucket}/versions?key={key}",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    let list_status = list.status();
    let list_text = list.text().await.unwrap();
    assert_eq!(list_status, 200, "list versions failed: {list_text}");
    let list_body: serde_json::Value = serde_json::from_str(&list_text).unwrap();
    let versions: Vec<serde_json::Value> = list_body["versions"].as_array().unwrap().clone();
    assert_eq!(versions.len(), 2);
    let old_version_id = versions
        .iter()
        .find(|v| v["versionId"].as_str() != Some(current_version_id.as_str()))
        .unwrap()["versionId"]
        .as_str()
        .unwrap()
        .to_string();

    let download = client()
        .get(format!(
            "{}/api/buckets/{bucket}/versions/{old_version_id}/download/{key}",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(download.status(), 200);
    assert_eq!(&download.bytes().await.unwrap()[..], b"version-one");

    let delete = client()
        .delete(format!(
            "{}/api/buckets/{bucket}/versions/{old_version_id}/objects/{key}",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);

    let list = client()
        .get(format!(
            "{}/api/buckets/{bucket}/versions?key={key}",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    let list_body: serde_json::Value = list.json().await.unwrap();
    let remaining: Vec<serde_json::Value> = list_body["versions"].as_array().unwrap().clone();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0]["versionId"].as_str().unwrap(),
        current_version_id
    );
}

#[tokio::test]
async fn test_console_orphan_meta_repair_api() {
    let server = start_server().await;
    let base_url = &*server;
    let bucket = "orphan-api-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/orphan.txt", base_url),
            b"orphan bytes".to_vec(),
        )
        .await
        .status(),
        200
    );

    tokio::fs::remove_file(
        std::path::Path::new(server.data_dir())
            .join("buckets")
            .join(bucket)
            .join("orphan.txt"),
    )
    .await
    .unwrap();

    let session = console_login(base_url).await;

    let scan = client()
        .get(format!("{}/api/maintenance/orphan-meta", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(scan.status(), 200);
    let scan_body: serde_json::Value = scan.json().await.unwrap();
    assert_eq!(scan_body["count"], 1);

    let repair = client()
        .post(format!("{}/api/maintenance/orphan-meta/repair", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(repair.status(), 200);
    let repair_body: serde_json::Value = repair.json().await.unwrap();
    assert_eq!(repair_body["removed"], 1);

    let scan = client()
        .get(format!("{}/api/maintenance/orphan-meta", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(scan.status(), 200);
    assert_eq!(scan.json::<serde_json::Value>().await.unwrap()["count"], 0);
}
