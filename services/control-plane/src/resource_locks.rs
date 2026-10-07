use serde_json::Value;
use std::collections::HashSet;

pub const DEFAULT_WAIT_TIMEOUT_SECONDS: i64 = 300;

pub fn validate_suite_resource_locks(definition: &Value) -> Result<(Vec<String>, i64), String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    if let Some(value) = definition.get("resource_locks") {
        let entries = value
            .as_array()
            .ok_or("Suite resource_locks must be an array of names")?;
        if entries.len() > 32 {
            return Err("A suite can declare at most 32 resource locks".to_string());
        }
        for entry in entries {
            let name = entry
                .as_str()
                .ok_or("Suite resource lock names must be strings")?
                .trim();
            if name.is_empty()
                || name.chars().count() > 128
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
            {
                return Err(
                    "Resource lock names must use 1–128 letters, digits, or _ - . : characters"
                        .to_string(),
                );
            }
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err("Suite resource lock names must be unique".to_string());
            }
            names.push(name.to_string());
        }
    }
    let timeout = match definition.get("lock_wait_timeout_seconds") {
        Some(value) => value
            .as_i64()
            .filter(|seconds| (1..=3600).contains(seconds))
            .ok_or("Lock wait timeout must be from 1 to 3600 seconds")?,
        None => DEFAULT_WAIT_TIMEOUT_SECONDS,
    };
    Ok((names, timeout))
}

#[cfg(test)]
mod tests {
    use super::{validate_suite_resource_locks, DEFAULT_WAIT_TIMEOUT_SECONDS};
    use serde_json::json;

    #[test]
    fn suite_locks_default_to_no_resources_and_a_five_minute_wait() {
        assert_eq!(
            validate_suite_resource_locks(&json!({})).unwrap(),
            (Vec::<String>::new(), DEFAULT_WAIT_TIMEOUT_SECONDS)
        );
    }

    #[test]
    fn suite_locks_accept_named_resources_and_a_bounded_timeout() {
        assert_eq!(
            validate_suite_resource_locks(&json!({
                "resource_locks": ["env:staging", "tenant:shared-account"],
                "lock_wait_timeout_seconds": 45
            }))
            .unwrap(),
            (
                vec![
                    "env:staging".to_string(),
                    "tenant:shared-account".to_string()
                ],
                45
            )
        );
    }

    #[test]
    fn suite_locks_reject_ambiguous_or_unsafe_definitions() {
        for definition in [
            json!({ "resource_locks": "env:staging" }),
            json!({ "resource_locks": ["env:staging", "ENV:STAGING"] }),
            json!({ "resource_locks": ["../shared"] }),
            json!({ "lock_wait_timeout_seconds": 0 }),
            json!({ "lock_wait_timeout_seconds": 3601 }),
        ] {
            assert!(validate_suite_resource_locks(&definition).is_err());
        }
    }
}
