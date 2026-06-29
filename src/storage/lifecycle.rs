use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleAction {
    ExpireObjects { days: u32 },
    NoncurrentVersionExpiration { noncurrent_days: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleRule {
    pub id: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    pub actions: Vec<LifecycleAction>,
}

pub fn rule_prefix(rule: &LifecycleRule) -> &str {
    rule.prefix.as_deref().unwrap_or("")
}

pub fn rule_matches_prefix(rule: &LifecycleRule, key: &str) -> bool {
    let prefix = rule_prefix(rule);
    prefix.is_empty() || key.starts_with(prefix)
}

pub fn action_cutoff(action: &LifecycleAction, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let days = match action {
        LifecycleAction::ExpireObjects { days } => *days,
        LifecycleAction::NoncurrentVersionExpiration { noncurrent_days } => *noncurrent_days,
    };
    if days == 0 {
        return None;
    }
    Some(now - Duration::days(days as i64))
}

pub fn lifecycle_rules_from_xml(
    config: &crate::xml::types::LifecycleConfiguration,
) -> Result<Vec<LifecycleRule>, String> {
    if config.rules.len() > 1000 {
        return Err("Lifecycle configuration cannot have more than 1000 rules".into());
    }

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(config.rules.len());

    for rule in &config.rules {
        if rule.id.is_empty() {
            return Err("Lifecycle rule ID is required".into());
        }
        if !seen.insert(rule.id.clone()) {
            return Err(format!("Duplicate lifecycle rule ID: {}", rule.id));
        }

        let enabled = match rule.status.as_str() {
            "Enabled" => true,
            "Disabled" => false,
            other => return Err(format!("Invalid lifecycle rule status: {other}")),
        };

        let prefix = rule
            .filter
            .as_ref()
            .and_then(|f| f.prefix.clone())
            .filter(|p| !p.is_empty());

        let mut actions = Vec::new();
        if let Some(exp) = &rule.expiration {
            if exp.days < 1 {
                return Err("Expiration Days must be at least 1".into());
            }
            actions.push(LifecycleAction::ExpireObjects { days: exp.days });
        }
        if let Some(nc) = &rule.noncurrent_version_expiration {
            if nc.noncurrent_days < 1 {
                return Err("NoncurrentDays must be at least 1".into());
            }
            actions.push(LifecycleAction::NoncurrentVersionExpiration {
                noncurrent_days: nc.noncurrent_days,
            });
        }
        if actions.is_empty() {
            return Err(format!(
                "Lifecycle rule {} must specify at least one action",
                rule.id
            ));
        }

        out.push(LifecycleRule {
            id: rule.id.clone(),
            enabled,
            prefix,
            actions,
        });
    }

    Ok(out)
}

pub fn lifecycle_rules_to_xml(
    rules: &[LifecycleRule],
) -> crate::xml::types::LifecycleConfiguration {
    use crate::xml::types::{
        LifecycleExpirationXml, LifecycleFilterXml, LifecycleRuleXml,
        NoncurrentVersionExpirationXml,
    };

    crate::xml::types::LifecycleConfiguration {
        rules: rules
            .iter()
            .map(|rule| {
                let mut expiration = None;
                let mut noncurrent_version_expiration = None;
                for action in &rule.actions {
                    match action {
                        LifecycleAction::ExpireObjects { days } => {
                            expiration = Some(LifecycleExpirationXml { days: *days });
                        }
                        LifecycleAction::NoncurrentVersionExpiration { noncurrent_days } => {
                            noncurrent_version_expiration = Some(NoncurrentVersionExpirationXml {
                                noncurrent_days: *noncurrent_days,
                            });
                        }
                    }
                }
                LifecycleRuleXml {
                    id: rule.id.clone(),
                    status: if rule.enabled {
                        "Enabled".to_string()
                    } else {
                        "Disabled".to_string()
                    },
                    filter: rule.prefix.as_ref().map(|prefix| LifecycleFilterXml {
                        prefix: Some(prefix.clone()),
                    }),
                    expiration,
                    noncurrent_version_expiration,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_matches_prefix_works() {
        let rule = LifecycleRule {
            id: "r1".into(),
            enabled: true,
            prefix: Some("logs/".into()),
            actions: vec![LifecycleAction::ExpireObjects { days: 30 }],
        };
        assert!(rule_matches_prefix(&rule, "logs/a.txt"));
        assert!(!rule_matches_prefix(&rule, "data/a.txt"));
    }

    #[test]
    fn action_cutoff_subtracts_days() {
        let now = Utc::now();
        let cutoff = action_cutoff(&LifecycleAction::ExpireObjects { days: 30 }, now).unwrap();
        assert!(cutoff < now);
    }
}
