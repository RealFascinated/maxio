use crate::common::*;

#[tokio::test]
async fn test_iam_create_user_and_access_key() {
    let base_url = start_server().await;

    let resp = iam_action(
        &base_url,
        &[("Action", "CreateUser"), ("UserName", "alice")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<UserName>alice</UserName>"));

    let resp = iam_action(
        &base_url,
        &[("Action", "CreateAccessKey"), ("UserName", "alice")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    let access_key_id = extract_xml_tag(&body, "AccessKeyId").expect("access key id");
    let secret = extract_xml_tag(&body, "SecretAccessKey").expect("secret key");

    let resp = s3_request_as(
        "GET",
        &format!("{}/", base_url),
        vec![],
        &access_key_id,
        &secret,
    )
    .await;
    assert_eq!(
        resp.status(),
        403,
        "new user without policy cannot list buckets"
    );
}

#[tokio::test]
async fn test_iam_user_policy_grants_object_read() {
    let base_url = start_server().await;
    let bucket = "iam-read-bucket";

    assert_eq!(
        s3_request("PUT", &format!("{}/{bucket}", base_url), vec![])
            .await
            .status(),
        200
    );
    assert_eq!(
        s3_request(
            "PUT",
            &format!("{}/{bucket}/secret.txt", base_url),
            b"classified".to_vec(),
        )
        .await
        .status(),
        200
    );

    assert_eq!(
        iam_action(
            &base_url,
            &[("Action", "CreateUser"), ("UserName", "reader")],
        )
        .await
        .status(),
        200
    );

    let key_resp = iam_action(
        &base_url,
        &[("Action", "CreateAccessKey"), ("UserName", "reader")],
    )
    .await;
    let key_body = key_resp.text().await.unwrap();
    let access_key_id = extract_xml_tag(&key_body, "AccessKeyId").unwrap();
    let secret = extract_xml_tag(&key_body, "SecretAccessKey").unwrap();

    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject"],"Resource":["arn:aws:s3:::iam-read-bucket/*"]}]}"#;
    assert_eq!(
        iam_action(
            &base_url,
            &[
                ("Action", "PutUserPolicy"),
                ("UserName", "reader"),
                ("PolicyName", "read-objects"),
                ("PolicyDocument", policy),
            ],
        )
        .await
        .status(),
        200
    );

    let get = s3_request_as(
        "GET",
        &format!("{}/{bucket}/secret.txt", base_url),
        vec![],
        &access_key_id,
        &secret,
    )
    .await;
    assert_eq!(get.status(), 200);
    assert_eq!(&get.bytes().await.unwrap()[..], b"classified");

    let put = s3_request_as(
        "PUT",
        &format!("{}/{bucket}/other.txt", base_url),
        b"nope".to_vec(),
        &access_key_id,
        &secret,
    )
    .await;
    assert_eq!(put.status(), 403);
}

#[tokio::test]
async fn test_iam_list_and_delete_user() {
    let base_url = start_server().await;

    assert_eq!(
        iam_action(
            &base_url,
            &[("Action", "CreateUser"), ("UserName", "list-me")],
        )
        .await
        .status(),
        200
    );

    let list = iam_action(&base_url, &[("Action", "ListUsers")]).await;
    assert_eq!(list.status(), 200);
    let body = list.text().await.unwrap();
    assert!(body.contains("<UserName>list-me</UserName>"));

    assert_eq!(
        iam_action(
            &base_url,
            &[("Action", "DeleteUser"), ("UserName", "list-me")],
        )
        .await
        .status(),
        200
    );

    let list_after = iam_action(&base_url, &[("Action", "ListUsers")]).await;
    let body_after = list_after.text().await.unwrap();
    assert!(!body_after.contains("<UserName>list-me</UserName>"));
}

#[tokio::test]
async fn test_iam_non_admin_denied() {
    let base_url = start_server().await;

    assert_eq!(
        iam_action(&base_url, &[("Action", "CreateUser"), ("UserName", "bob")],)
            .await
            .status(),
        200
    );

    let key_resp = iam_action(
        &base_url,
        &[("Action", "CreateAccessKey"), ("UserName", "bob")],
    )
    .await;
    let key_body = key_resp.text().await.unwrap();
    let access_key_id = extract_xml_tag(&key_body, "AccessKeyId").unwrap();
    let secret = extract_xml_tag(&key_body, "SecretAccessKey").unwrap();

    let body = "Action=CreateUser&UserName=carol";
    let denied = s3_request_as(
        "POST",
        &format!("{base_url}/iam"),
        body.as_bytes().to_vec(),
        &access_key_id,
        &secret,
    )
    .await;
    assert_eq!(denied.status(), 403);
}
