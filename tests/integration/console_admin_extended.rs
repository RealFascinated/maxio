use crate::common::*;

const READ_OBJECTS_POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject","s3:ListBucket"],"Resource":["arn:aws:s3:::scoped-bucket","arn:aws:s3:::scoped-bucket/*"]}]}"#;

async fn create_console_user_with_policy(
    base_url: &str,
    session: &str,
    username: &str,
    policy: &str,
) -> (String, String) {
    let create = client()
        .post(format!("{base_url}/api/users"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", base_url)
        .json(&serde_json::json!({ "username": username }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);
    let body: serde_json::Value = create.json().await.unwrap();
    let access_key = body["accessKey"]["accessKeyId"]
        .as_str()
        .unwrap()
        .to_string();
    let secret = body["accessKey"]["secretAccessKey"]
        .as_str()
        .unwrap()
        .to_string();

    let put_policy = client()
        .put(format!(
            "{base_url}/api/users/{username}/policies/read-access"
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", base_url)
        .json(&serde_json::json!({ "document": policy }))
        .send()
        .await
        .unwrap();
    assert_eq!(put_policy.status(), 200);

    (access_key, secret)
}

#[tokio::test]
async fn test_console_admin_delete_user() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let create = client()
        .post(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "username": "delete-me" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let delete = client()
        .delete(format!("{}/api/users/delete-me", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);

    let list = client()
        .get(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list.json().await.unwrap();
    let names: Vec<String> = list_body["users"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|u| u["username"].as_str().map(String::from))
        .collect();
    assert!(!names.iter().any(|n| n == "delete-me"));
}

#[tokio::test]
async fn test_console_admin_access_key_lifecycle() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let create = client()
        .post(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "username": "key-user" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let new_key = client()
        .post(format!("{}/api/users/key-user/keys", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(new_key.status(), 200);
    let key_body: serde_json::Value = new_key.json().await.unwrap();
    let access_key_id = key_body["accessKeyId"].as_str().unwrap().to_string();

    let list = client()
        .get(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list.json().await.unwrap();
    let user = list_body["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "key-user")
        .unwrap()
        .clone();
    let key_ids: Vec<String> = user["accessKeys"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|k| k["accessKeyId"].as_str().map(String::from))
        .collect();
    assert!(key_ids.iter().any(|id| id == &access_key_id));

    let delete_key = client()
        .delete(format!(
            "{}/api/users/key-user/keys/{access_key_id}",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete_key.status(), 200);

    let login = client()
        .post(format!("{}/api/auth/login", base_url))
        .json(&serde_json::json!({
            "accessKey": access_key_id,
            "secretKey": key_body["secretAccessKey"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 401);
}

#[tokio::test]
async fn test_console_admin_managed_policy_crud() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:ListBucket"],"Resource":["arn:aws:s3:::*"]}]}"#;

    let create = client()
        .post(format!("{}/api/policies", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({
            "name": "list-all-buckets",
            "document": policy_doc,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);
    let created: serde_json::Value = create.json().await.unwrap();
    let arn = created["arn"].as_str().unwrap().to_string();

    let list = client()
        .get(format!("{}/api/policies", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list.json().await.unwrap();
    let names: Vec<String> = list_body["policies"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str().map(String::from))
        .collect();
    assert!(names.iter().any(|n| n == "list-all-buckets"));

    let get = client()
        .get(format!("{}/api/policies/list-all-buckets", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    assert_eq!(get.json::<serde_json::Value>().await.unwrap()["arn"], arn);

    let delete = client()
        .delete(format!("{}/api/policies/list-all-buckets", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);
}

#[tokio::test]
async fn test_console_admin_user_inline_policy() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject"],"Resource":["arn:aws:s3:::inline-bucket/*"]}]}"#;

    let create = client()
        .post(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "username": "inline-user" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let put = client()
        .put(format!(
            "{}/api/users/inline-user/policies/read-inline",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "document": policy_doc }))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 200);

    let get = client()
        .get(format!(
            "{}/api/users/inline-user/policies/read-inline",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let get_body: serde_json::Value = get.json().await.unwrap();
    assert!(
        get_body["document"]
            .as_str()
            .unwrap()
            .contains("inline-bucket")
    );

    let delete = client()
        .delete(format!(
            "{}/api/users/inline-user/policies/read-inline",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 200);

    let get = client()
        .get(format!(
            "{}/api/users/inline-user/policies/read-inline",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 404);
}

#[tokio::test]
async fn test_console_admin_user_inline_policy_rejects_invalid_document() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let invalid_policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Maybe","Action":["s3:GetObject"],"Resource":["*"]}]}"#;

    let create = client()
        .post(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "username": "invalid-policy-user" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let put = client()
        .put(format!(
            "{}/api/users/invalid-policy-user/policies/bad-inline",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "document": invalid_policy }))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 400);
    let body: serde_json::Value = put.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("invalid Effect"),
        "expected semantic policy error, got {}",
        body["error"]
    );

    let get = client()
        .get(format!(
            "{}/api/users/invalid-policy-user/policies/bad-inline",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 404);
}

#[tokio::test]
async fn test_console_admin_attach_detach_policy() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;
    let policy_doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:ListBucket"],"Resource":["arn:aws:s3:::attach-bucket"]}]}"#;

    let create_policy = client()
        .post(format!("{}/api/policies", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({
            "name": "attach-policy",
            "document": policy_doc,
        }))
        .send()
        .await
        .unwrap();
    let created_body: serde_json::Value = create_policy.json().await.unwrap();
    let arn = created_body["arn"].as_str().unwrap().to_string();

    let create_user = client()
        .post(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "username": "attach-user" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_user.status(), 200);

    let attach = client()
        .post(format!("{}/api/users/attach-user/attach-policy", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "policyArn": arn }))
        .send()
        .await
        .unwrap();
    assert_eq!(attach.status(), 200);

    let list = client()
        .get(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list.json().await.unwrap();
    let user = list_body["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "attach-user")
        .unwrap();
    assert!(
        user["attachedPolicies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str() == Some(arn.as_str()))
    );

    let detach = client()
        .post(format!("{}/api/users/attach-user/detach-policy", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "policyArn": arn }))
        .send()
        .await
        .unwrap();
    assert_eq!(detach.status(), 200);

    let list = client()
        .get(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    let list_body: serde_json::Value = list.json().await.unwrap();
    let user = list_body["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "attach-user")
        .unwrap();
    assert!(user["attachedPolicies"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_console_iam_user_login() {
    let base_url = start_server().await;
    let root_session = console_login(&base_url).await;

    let (access_key, secret) = create_console_user_with_policy(
        &base_url,
        &root_session,
        "console-iam-user",
        READ_OBJECTS_POLICY,
    )
    .await;

    let login = client()
        .post(format!("{}/api/auth/login", base_url))
        .json(&serde_json::json!({
            "accessKey": access_key,
            "secretKey": secret,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let body: serde_json::Value = login.json().await.unwrap();
    assert_eq!(body["username"], "console-iam-user");
    assert_eq!(body["isRoot"], false);
    assert_eq!(body["capabilities"]["canManageUsers"], false);
}

#[tokio::test]
async fn test_console_scoped_access() {
    let base_url = start_server().await;
    let root_session = console_login(&base_url).await;

    assert_eq!(
        s3_request("PUT", &format!("{}/scoped-bucket", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/scoped-bucket/allowed.txt", base_url),
            b"allowed".to_vec(),
        )
        .await
        .status(),
        200
    );
    assert_eq!(
        s3_request("PUT", &format!("{}/other-bucket", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/other-bucket/secret.txt", base_url),
            b"secret".to_vec(),
        )
        .await
        .status(),
        200
    );

    let (access_key, secret) = create_console_user_with_policy(
        &base_url,
        &root_session,
        "scoped-user",
        READ_OBJECTS_POLICY,
    )
    .await;
    let session = console_login_with_creds(&base_url, &access_key, &secret).await;

    let buckets = client()
        .get(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(buckets.status(), 200);
    let buckets_body: serde_json::Value = buckets.json().await.unwrap();
    let names: Vec<String> = buckets_body["buckets"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["name"].as_str().map(String::from))
        .collect();
    assert_eq!(names, vec!["scoped-bucket".to_string()]);

    let allowed = client()
        .get(format!(
            "{}/api/buckets/scoped-bucket/objects?prefix=",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status(), 200);

    let denied = client()
        .get(format!(
            "{}/api/buckets/other-bucket/objects?prefix=",
            base_url
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let admin = client()
        .get(format!("{}/api/users", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(admin.status(), 403);
}

const CREATE_BUCKET_POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:CreateBucket"],"Resource":"arn:aws:s3:::*"}]}"#;

const DENY_ALL_BUCKET_POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Deny","Action":"s3:*","Resource":["arn:aws:s3:::owner-console-bucket","arn:aws:s3:::owner-console-bucket/*"],"Principal":"*"}]}"#;

#[tokio::test]
async fn test_console_bucket_owner_bypasses_deny_policy() {
    let base_url = start_server().await;
    let root_session = console_login(&base_url).await;

    let (access_key, secret) = create_console_user_with_policy(
        &base_url,
        &root_session,
        "bucket-owner",
        CREATE_BUCKET_POLICY,
    )
    .await;
    let session = console_login_with_creds(&base_url, &access_key, &secret).await;

    let create = client()
        .post(format!("{base_url}/api/buckets"))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "name": "owner-console-bucket" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);

    let put_policy = s3_request(
        "PUT",
        &format!("{base_url}/owner-console-bucket?policy"),
        DENY_ALL_BUCKET_POLICY.as_bytes().to_vec(),
    )
    .await;
    assert!(put_policy.status().is_success());

    assert_eq!(
        s3_request_as(
            "PUT",
            &format!("{base_url}/owner-console-bucket/owned.txt"),
            b"owned by iam user".to_vec(),
            &access_key,
            &secret,
        )
        .await
        .status(),
        200
    );

    let download = client()
        .get(format!(
            "{base_url}/api/buckets/owner-console-bucket/download/owned.txt"
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(download.status(), 200);
    assert_eq!(
        download.bytes().await.unwrap().as_ref(),
        b"owned by iam user"
    );
}

#[tokio::test]
async fn test_console_object_acl_grants_access() {
    let base_url = start_server().await;
    let root_session = console_login(&base_url).await;
    let bucket = "obj-acl-console-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{base_url}/{bucket}"), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{base_url}/{bucket}/shared.txt"),
            b"shared via object acl".to_vec(),
        )
        .await
        .status(),
        200
    );
    assert_eq!(
        s3_request_with_headers(
            "PUT",
            &format!("{base_url}/{bucket}/shared.txt?acl"),
            vec![],
            vec![("x-amz-acl", "authenticated-read")],
        )
        .await
        .status(),
        200
    );

    let create = client()
        .post(format!("{base_url}/api/users"))
        .header("cookie", format!("maxio_session={root_session}"))
        .header("origin", &*base_url)
        .json(&serde_json::json!({ "username": "acl-reader" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 200);
    let create_body: serde_json::Value = create.json().await.unwrap();
    let access_key = create_body["accessKey"]["accessKeyId"]
        .as_str()
        .unwrap()
        .to_string();
    let secret = create_body["accessKey"]["secretAccessKey"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(
        s3_request_as(
            "GET",
            &format!("{base_url}/{bucket}/shared.txt"),
            vec![],
            &access_key,
            &secret,
        )
        .await
        .status(),
        200
    );

    let session = console_login_with_creds(&base_url, &access_key, &secret).await;
    let download = client()
        .get(format!(
            "{base_url}/api/buckets/{bucket}/download/shared.txt"
        ))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(download.status(), 200);
    assert_eq!(
        download.bytes().await.unwrap().as_ref(),
        b"shared via object acl"
    );
}

#[tokio::test]
async fn test_console_login_rate_limit() {
    let base_url = start_server().await;

    for _ in 0..10 {
        let resp = client()
            .post(format!("{}/api/auth/login", base_url))
            .json(&serde_json::json!({
                "accessKey": "bad",
                "secretKey": "bad",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    let resp = client()
        .post(format!("{}/api/auth/login", base_url))
        .json(&serde_json::json!({
            "accessKey": "bad",
            "secretKey": "bad",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    assert!(resp.headers().get("retry-after").is_some());
}

#[tokio::test]
async fn test_console_revoked_session_rejected() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let logout = client()
        .post(format!("{}/api/auth/logout", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .header("origin", &*base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 200);

    let buckets = client()
        .get(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(buckets.status(), 401);
}
