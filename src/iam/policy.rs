use super::principal::Principal;
use super::types::{PolicyDocumentRaw, PrincipalSpec, StatementRaw};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Debug, Clone)]
pub struct Statement {
    pub effect: Effect,
    pub actions: Vec<String>,
    pub resources: Vec<String>,
    pub principal: Option<PrincipalSpec>,
}

#[derive(Debug, Clone)]
pub struct PolicyDocument {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthDecision {
    Allow,
    Deny,
    NoMatch,
}

pub fn parse_policy_document(raw: &PolicyDocumentRaw) -> Result<PolicyDocument, String> {
    let statements = raw
        .statement
        .iter()
        .map(|s| {
            let effect = match s.effect.to_ascii_lowercase().as_str() {
                "allow" => Effect::Allow,
                "deny" => Effect::Deny,
                other => return Err(format!("invalid Effect: {other}")),
            };
            Ok(Statement {
                effect,
                actions: s.action.clone(),
                resources: s.resource.clone(),
                principal: s.principal.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(PolicyDocument { statements })
}

pub fn parse_policy_json(json: &str) -> Result<PolicyDocument, String> {
    let raw: PolicyDocumentRaw =
        serde_json::from_str(json).map_err(|e| format!("invalid policy JSON: {e}"))?;
    parse_policy_document(&raw)
}

fn is_supported_policy_version(version: &str) -> bool {
    version == "2012-10-17" || version == "2008-10-17"
}

fn validate_principal_for_bucket_policy(principal: &Option<PrincipalSpec>) -> Result<(), String> {
    let Some(principal) = principal else {
        return Err("bucket policy statement requires Principal".into());
    };
    match principal {
        PrincipalSpec::Star(s) if s == "*" => Ok(()),
        PrincipalSpec::Star(_) => Err("invalid Principal format".into()),
        PrincipalSpec::Map(map) => {
            if map.star {
                return Ok(());
            }
            if map.aws.is_empty() {
                return Err("Principal.AWS must not be empty".into());
            }
            for arn in &map.aws {
                if arn == "*" {
                    continue;
                }
                if arn.starts_with("arn:aws:iam::") || arn.starts_with("arn:aws:sts::") {
                    continue;
                }
                return Err(format!("invalid Principal ARN: {arn}"));
            }
            Ok(())
        }
    }
}

/// Whether a bucket policy Resource element may refer to `bucket`.
pub fn resource_allowed_for_bucket(resource: &str, bucket: &str) -> bool {
    if resource == "*" {
        return true;
    }
    const PREFIX: &str = "arn:aws:s3:::";
    if !resource.starts_with(PREFIX) {
        return false;
    }
    let rest = &resource[PREFIX.len()..];
    if rest.is_empty() {
        return false;
    }
    let bucket_part = rest.split('/').next().unwrap_or(rest);
    if bucket_part == bucket {
        return true;
    }
    glob_match(bucket_part, bucket)
}

/// Validate a bucket policy document before PutBucketPolicy.
pub fn validate_bucket_policy_for_put(json: &str, bucket: &str) -> Result<PolicyDocument, String> {
    let raw: PolicyDocumentRaw =
        serde_json::from_str(json).map_err(|e| format!("invalid policy JSON: {e}"))?;

    if !is_supported_policy_version(&raw.version) {
        return Err(format!(
            "unsupported policy Version: {} (expected 2012-10-17 or 2008-10-17)",
            raw.version
        ));
    }

    if raw.statement.is_empty() {
        return Err("policy must contain at least one Statement".into());
    }

    for (i, stmt) in raw.statement.iter().enumerate() {
        validate_principal_for_bucket_policy(&stmt.principal)
            .map_err(|e| format!("Statement[{i}]: {e}"))?;

        if stmt.action.is_empty() {
            return Err(format!("Statement[{i}]: Action must not be empty"));
        }
        if stmt.resource.is_empty() {
            return Err(format!("Statement[{i}]: Resource must not be empty"));
        }
        for resource in &stmt.resource {
            if !resource_allowed_for_bucket(resource, bucket) {
                return Err(format!(
                    "Statement[{i}]: Resource {resource} is not allowed for bucket {bucket}"
                ));
            }
        }
        if let Some(condition) = &stmt.condition {
            if !condition.is_object() {
                return Err(format!("Statement[{i}]: Condition must be a JSON object"));
            }
        }
    }

    parse_policy_document(&raw)
}

/// Evaluate whether `principal` may perform `action` on `resource`.
pub fn evaluate(
    principal: &Principal,
    action: &str,
    resource: &str,
    identity_policies: &[PolicyDocument],
    bucket_policy: Option<&PolicyDocument>,
) -> AuthDecision {
    if principal.is_root {
        return AuthDecision::Allow;
    }

    let mut explicit_deny = false;
    let mut identity_allow = false;
    let mut bucket_allow = false;

    for doc in identity_policies {
        match eval_document(doc, principal, action, resource, false) {
            AuthDecision::Deny => explicit_deny = true,
            AuthDecision::Allow => identity_allow = true,
            AuthDecision::NoMatch => {}
        }
    }

    if let Some(doc) = bucket_policy {
        match eval_document(doc, principal, action, resource, true) {
            AuthDecision::Deny => explicit_deny = true,
            AuthDecision::Allow => bucket_allow = true,
            AuthDecision::NoMatch => {}
        }
    }

    if explicit_deny {
        return AuthDecision::Deny;
    }
    if identity_allow || bucket_allow {
        return AuthDecision::Allow;
    }
    AuthDecision::NoMatch
}

fn eval_document(
    doc: &PolicyDocument,
    principal: &Principal,
    action: &str,
    resource: &str,
    is_bucket_policy: bool,
) -> AuthDecision {
    let mut deny = false;
    let mut allow = false;

    for stmt in &doc.statements {
        if is_bucket_policy {
            if stmt.principal.is_none() || !principal_matches(&stmt.principal, principal) {
                continue;
            }
        } else if stmt.principal.is_some() {
            // Identity policies should not have Principal
            continue;
        }

        if !action_matches(action, &stmt.actions) {
            continue;
        }
        if !resource_matches(resource, &stmt.resources) {
            continue;
        }

        match stmt.effect {
            Effect::Deny => deny = true,
            Effect::Allow => allow = true,
        }
    }

    if deny {
        AuthDecision::Deny
    } else if allow {
        AuthDecision::Allow
    } else {
        AuthDecision::NoMatch
    }
}

fn principal_matches(spec: &Option<PrincipalSpec>, principal: &Principal) -> bool {
    let Some(spec) = spec else {
        return true;
    };
    match spec {
        PrincipalSpec::Star(s) if s == "*" => true,
        PrincipalSpec::Map(map) => {
            if map.star {
                return true;
            }
            map.aws.iter().any(|p| {
                p == "*"
                    || p == &principal.arn()
                    || p.ends_with(&format!(":user/{}", principal.username))
            })
        }
        _ => false,
    }
}

fn action_matches(action: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_match(p, action))
}

fn resource_matches(resource: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_match(p, resource))
}

fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let re_pattern = regex_from_glob(pattern);
    glob_regex_match(&re_pattern, value)
}

fn regex_from_glob(pattern: &str) -> Vec<GlobPart> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    for ch in pattern.chars() {
        match ch {
            '*' => {
                if !literal.is_empty() {
                    parts.push(GlobPart::Literal(std::mem::take(&mut literal)));
                }
                parts.push(GlobPart::Star);
            }
            '?' => {
                if !literal.is_empty() {
                    parts.push(GlobPart::Literal(std::mem::take(&mut literal)));
                }
                parts.push(GlobPart::Question);
            }
            _ => literal.push(ch),
        }
    }
    if !literal.is_empty() {
        parts.push(GlobPart::Literal(literal));
    }
    parts
}

#[derive(Debug)]
enum GlobPart {
    Literal(String),
    Star,
    Question,
}

fn glob_regex_match(parts: &[GlobPart], value: &str) -> bool {
    fn rec(parts: &[GlobPart], value: &str) -> bool {
        if parts.is_empty() {
            return value.is_empty();
        }
        match &parts[0] {
            GlobPart::Literal(lit) => {
                if value.starts_with(lit) {
                    rec(&parts[1..], &value[lit.len()..])
                } else {
                    false
                }
            }
            GlobPart::Question => {
                if value.is_empty() {
                    false
                } else {
                    let mut chars = value.chars();
                    chars.next();
                    rec(&parts[1..], chars.as_str())
                }
            }
            GlobPart::Star => {
                if parts.len() == 1 {
                    return true;
                }
                let len = value.len();
                for i in 0..=len {
                    if rec(&parts[1..], &value[i..]) {
                        return true;
                    }
                }
                false
            }
        }
    }
    rec(parts, value)
}

/// Build a bucket policy document for console public read/list toggles.
pub fn public_read_policy(bucket: &str) -> PolicyDocumentRaw {
    PolicyDocumentRaw {
        version: "2012-10-17".to_string(),
        statement: vec![StatementRaw {
            sid: Some("PublicRead".to_string()),
            effect: "Allow".to_string(),
            action: vec!["s3:GetObject".to_string()],
            resource: vec![format!("arn:aws:s3:::{bucket}/*")],
            principal: Some(PrincipalSpec::Star("*".to_string())),
            condition: None,
        }],
    }
}

pub fn public_list_policy(bucket: &str) -> PolicyDocumentRaw {
    PolicyDocumentRaw {
        version: "2012-10-17".to_string(),
        statement: vec![StatementRaw {
            sid: Some("PublicList".to_string()),
            effect: "Allow".to_string(),
            action: vec!["s3:ListBucket".to_string()],
            resource: vec![format!("arn:aws:s3:::{bucket}")],
            principal: Some(PrincipalSpec::Star("*".to_string())),
            condition: None,
        }],
    }
}

pub fn merge_public_access_policy(
    bucket: &str,
    existing: Option<&str>,
    public_read: bool,
    public_list: bool,
) -> Result<String, String> {
    let mut statements: Vec<StatementRaw> = existing
        .and_then(|p| serde_json::from_str::<PolicyDocumentRaw>(p).ok())
        .map(|d| d.statement)
        .unwrap_or_default();

    statements.retain(|s| {
        s.sid.as_deref() != Some("PublicRead") && s.sid.as_deref() != Some("PublicList")
    });

    if public_read {
        statements.extend(public_read_policy(bucket).statement);
    }
    if public_list {
        statements.extend(public_list_policy(bucket).statement);
    }

    let doc = PolicyDocumentRaw {
        version: "2012-10-17".to_string(),
        statement: statements,
    };
    let json = serde_json::to_string(&doc).map_err(|e| e.to_string())?;
    validate_bucket_policy_for_put(&json, bucket)?;
    Ok(json)
}

pub fn policy_has_public_read(policy_json: Option<&str>) -> bool {
    policy_json
        .and_then(|p| parse_policy_json(p).ok())
        .map(|doc| {
            doc.statements.iter().any(|s| {
                s.effect == Effect::Allow
                    && s.actions
                        .iter()
                        .any(|a| a == "s3:GetObject" || a == "s3:*" || a == "*")
                    && s.principal.is_some()
            })
        })
        .unwrap_or(false)
}

pub fn policy_has_public_list(policy_json: Option<&str>) -> bool {
    policy_json
        .and_then(|p| parse_policy_json(p).ok())
        .map(|doc| {
            doc.statements.iter().any(|s| {
                s.effect == Effect::Allow
                    && s.actions
                        .iter()
                        .any(|a| a == "s3:ListBucket" || a == "s3:*" || a == "*")
                    && s.principal.is_some()
            })
        })
        .unwrap_or(false)
}
