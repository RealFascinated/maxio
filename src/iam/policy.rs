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
    #[allow(dead_code)]
    pub version: String,
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
    Ok(PolicyDocument {
        version: raw.version.clone(),
        statements,
    })
}

pub fn parse_policy_json(json: &str) -> Result<PolicyDocument, String> {
    let raw: PolicyDocumentRaw =
        serde_json::from_str(json).map_err(|e| format!("invalid policy JSON: {e}"))?;
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
            if !principal_matches(&stmt.principal, principal) {
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

/// Build a public-read bucket policy from legacy flags.
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
    serde_json::to_string(&doc).map_err(|e| e.to_string())
}

pub fn policy_has_public_read(policy_json: Option<&str>) -> bool {
    policy_json
        .and_then(|p| parse_policy_json(p).ok())
        .map(|doc| {
            doc.statements.iter().any(|s| {
                s.effect == Effect::Allow
                    && s.actions.iter().any(|a| a == "s3:GetObject" || a == "s3:*" || a == "*")
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
                    && s.actions.iter().any(|a| a == "s3:ListBucket" || a == "s3:*" || a == "*")
                    && s.principal.is_some()
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iam::principal::Principal;
    use crate::iam::types::{PolicyDocumentRaw, StatementRaw};

    #[test]
    fn allow_read_object() {
        let doc = parse_policy_document(&PolicyDocumentRaw {
            version: "2012-10-17".into(),
            statement: vec![StatementRaw {
                sid: None,
                effect: "Allow".to_string(),
                action: vec!["s3:GetObject".to_string()],
                resource: vec!["arn:aws:s3:::my-bucket/*".to_string()],
                principal: None,
                condition: None,
            }],
        })
        .unwrap();
        let p = Principal {
            username: "alice".into(),
            user_id: "AIDA123".into(),
            display_name: "alice".into(),
            canonical_id: "AIDA123".into(),
            is_root: false,
            is_anonymous: false,
        };
        assert_eq!(
            evaluate(&p, "s3:GetObject", "arn:aws:s3:::my-bucket/file.txt", &[doc], None),
            AuthDecision::Allow
        );
    }

    #[test]
    fn deny_overrides_allow() {
        let allow = parse_policy_document(&PolicyDocumentRaw {
            version: "2012-10-17".into(),
            statement: vec![StatementRaw {
                sid: None,
                effect: "Allow".to_string(),
                action: vec!["s3:*".to_string()],
                resource: vec!["*".to_string()],
                principal: None,
                condition: None,
            }],
        })
        .unwrap();
        let deny = parse_policy_document(&PolicyDocumentRaw {
            version: "2012-10-17".into(),
            statement: vec![StatementRaw {
                sid: None,
                effect: "Deny".to_string(),
                action: vec!["s3:DeleteObject".to_string()],
                resource: vec!["arn:aws:s3:::secret/*".to_string()],
                principal: None,
                condition: None,
            }],
        })
        .unwrap();
        let p = Principal {
            username: "alice".into(),
            user_id: "AIDA123".into(),
            display_name: "alice".into(),
            canonical_id: "AIDA123".into(),
            is_root: false,
            is_anonymous: false,
        };
        assert_eq!(
            evaluate(
                &p,
                "s3:DeleteObject",
                "arn:aws:s3:::secret/key",
                &[allow, deny],
                None
            ),
            AuthDecision::Deny
        );
    }
}
