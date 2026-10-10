use super::*;

#[test]
fn parses_bedrock_cost_only() {
    let page = json!({
        "ResultsByTime": [{
            "Groups": [
                {
                    "Keys": ["Amazon Bedrock"],
                    "Metrics": { "UnblendedCost": { "Amount": "12.34" } }
                },
                {
                    "Keys": ["Amazon S3"],
                    "Metrics": { "UnblendedCost": { "Amount": "99.00" } }
                }
            ]
        }]
    });
    assert_eq!(parse_bedrock_cost(&page), 12.34);
}

#[test]
fn parses_cloudwatch_claude_activity() {
    let activity = parse_claude_activity(&json!({
        "MetricDataResults": [
            {"Id": "input", "Values": [10, 15]},
            {"Id": "output", "Values": [7]},
            {"Id": "requests", "Values": [2, 3]}
        ]
    }));
    assert_eq!(activity.input_tokens, 25.0);
    assert_eq!(activity.output_tokens, 7.0);
    assert_eq!(activity.request_count, 5.0);
}

#[test]
fn parses_context_credentials_from_json() {
    let credentials = BedrockProvider::credentials_from_context(Some(
        r#"{
                "access_key_id": "AKIAEXAMPLE",
                "secret_access_key": "secret",
                "session_token": "session"
            }"#,
    ))
    .expect("credentials");

    assert_eq!(credentials.access_key_id, "AKIAEXAMPLE");
    assert_eq!(credentials.secret_access_key, "secret");
    assert_eq!(credentials.session_token.as_deref(), Some("session"));
}

#[test]
fn parses_context_credentials_from_colon_delimited_value() {
    let credentials = BedrockProvider::credentials_from_context(Some("AKIAEXAMPLE:secret:session"))
        .expect("credentials");

    assert_eq!(credentials.access_key_id, "AKIAEXAMPLE");
    assert_eq!(credentials.secret_access_key, "secret");
    assert_eq!(credentials.session_token.as_deref(), Some("session"));
}

#[test]
fn parses_profile_from_context_prefix() {
    assert_eq!(
        BedrockProvider::profile_from_context(Some("profile:production")).as_deref(),
        Some("production")
    );
    assert!(BedrockProvider::credentials_from_context(Some("profile:production")).is_none());
}

#[test]
fn parses_profile_from_context_json() {
    assert_eq!(
        BedrockProvider::profile_from_context(Some(r#"{"aws_profile":"sso-dev"}"#)).as_deref(),
        Some("sso-dev")
    );
    assert!(
        BedrockProvider::credentials_from_context(Some(r#"{"aws_profile":"sso-dev"}"#)).is_none()
    );
}

#[test]
fn parses_aws_cli_export_credentials_output() {
    let credentials = parse_aws_profile_credentials(
        br#"{
                "Version": 1,
                "AccessKeyId": "ASIAEXAMPLE",
                "SecretAccessKey": "secret",
                "SessionToken": "session"
            }"#,
    )
    .expect("aws profile credentials");

    assert_eq!(credentials.access_key_id, "ASIAEXAMPLE");
    assert_eq!(credentials.secret_access_key, "secret");
    assert_eq!(credentials.session_token.as_deref(), Some("session"));
}

/// Golden SigV4 Authorization values pinned from the current signer, one
/// per canonical-header shape (with and without a session token).
#[test]
fn sigv4_authorization_matches_golden_values() {
    let body = br#"{"Granularity":"MONTHLY"}"#;
    let body_hash = sha256_hex(body);
    let mut credentials = AwsCredentials {
        access_key_id: "AKIDEXAMPLE".to_string(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
        session_token: None,
    };
    let request = AwsSigningRequest {
        date_stamp: "20260115",
        amz_date: "20260115T123456Z",
        body_hash: &body_hash,
        url: COST_EXPLORER_URL,
        body,
        target: COST_EXPLORER_TARGET,
        region: SIGNING_REGION,
        service: SERVICE,
    };
    let cost_explorer = sign_authorization_for(&credentials, request).unwrap();
    assert_eq!(
        cost_explorer,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260115/us-east-1/ce/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date;x-amz-target, Signature=bd41e425427d67f7b3e3979d3f1f617285addc2af80ebd24990b1be60c2cbaa2"
    );

    credentials.session_token = Some("session-token-example".to_string());
    let request = AwsSigningRequest {
        date_stamp: "20260115",
        amz_date: "20260115T123456Z",
        body_hash: &body_hash,
        url: "https://monitoring.eu-west-1.amazonaws.com",
        body,
        target: CLOUDWATCH_TARGET,
        region: "eu-west-1",
        service: CLOUDWATCH_SERVICE,
    };
    let cloudwatch = sign_authorization_for(&credentials, request).unwrap();
    assert_eq!(
        cloudwatch,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260115/eu-west-1/monitoring/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-target, Signature=ed9dd8a45f767b557cc7f86b1870456e2fe9c93d908b35d12e1024165cd263ca"
    );
}

#[test]
fn hmac_sha256_matches_rfc_4231_case_1() {
    let digest = hmac_sha256(&[0x0b; 20], b"Hi There");
    assert_eq!(
        hex(&digest),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
}
