use chrono::{DateTime, Duration, Utc};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolutionContext {
    pub run_id: String,
    pub suite_id: String,
    pub case_id: String,
    pub iteration_id: String,
    pub started_at_utc: String,
    pub seed: u64,
    pub run_vars: HashMap<String, Value>,
    pub env_vars: HashMap<String, Value>,
    pub suite_vars: HashMap<String, Value>,
    pub case_vars: HashMap<String, Value>,
    pub iteration_vars: HashMap<String, Value>,
    pub step_outputs: HashMap<String, HashMap<String, Value>>,
    pub secrets: HashMap<String, String>, // Key -> Secret name or placeholder
}

impl Default for ResolutionContext {
    fn default() -> Self {
        Self {
            run_id: uuid::Uuid::new_v4().to_string(),
            suite_id: uuid::Uuid::new_v4().to_string(),
            case_id: uuid::Uuid::new_v4().to_string(),
            iteration_id: "iter-0".to_string(),
            started_at_utc: Utc::now().to_rfc3339(),
            seed: 42,
            run_vars: HashMap::new(),
            env_vars: HashMap::new(),
            suite_vars: HashMap::new(),
            case_vars: HashMap::new(),
            iteration_vars: HashMap::new(),
            step_outputs: HashMap::new(),
            secrets: HashMap::new(),
        }
    }
}

pub struct VariableResolver<'a> {
    context: &'a ResolutionContext,
    rng: StdRng,
}

pub fn resolve_variable_definitions(
    definitions: Option<&Value>,
    context: &ResolutionContext,
    scope: &str,
) -> Result<HashMap<String, Value>, String> {
    let Some(definitions) = definitions else {
        return Ok(HashMap::new());
    };
    let object = definitions
        .as_object()
        .ok_or_else(|| "Variable definitions must be a JSON object".to_string())?;
    if object.len() > 100
        || serde_json::to_vec(definitions).map_or(true, |bytes| bytes.len() > 65_536)
    {
        return Err("Variable definitions exceed the configured count or size limit".to_string());
    }
    let mut dynamic_context = context.clone();
    let mut resolved = HashMap::new();
    for (name, definition) in object {
        if name.trim().is_empty() || name.len() > 128 {
            return Err("Variable names must contain 1 to 128 characters".to_string());
        }
        let value = if let Some(function) = definition.get("function").and_then(Value::as_str) {
            let args = definition
                .get("args")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("Variable '{}' requires a function argument array", name))?;
            if args.len() > 10 {
                return Err(format!(
                    "Variable '{}' has too many function arguments",
                    name
                ));
            }
            let mut function_context = dynamic_context.clone();
            function_context.seed = name.bytes().fold(dynamic_context.seed, |seed, byte| {
                seed.wrapping_mul(31).wrapping_add(byte as u64)
            });
            let mut resolver = VariableResolver::new(&function_context);
            let resolved_args = args
                .iter()
                .map(|arg| resolver.resolve_json_value(arg))
                .collect::<Result<Vec<_>, _>>()?;
            resolver.evaluate_function(function, &resolved_args)?
        } else if let (Some(kind), Some(value)) = (
            definition.get("type").and_then(Value::as_str),
            definition.get("value"),
        ) {
            validate_variable_type(kind, value)
                .map_err(|error| format!("Variable '{}': {}", name, error))?;
            value.clone()
        } else {
            definition.clone()
        };
        if serde_json::to_vec(&value).map_or(true, |bytes| bytes.len() > 65_536) {
            return Err(format!("Variable '{}' exceeds the value size limit", name));
        }
        match scope {
            "env" => {
                dynamic_context.env_vars.insert(name.clone(), value.clone());
            }
            "run" => {
                dynamic_context.run_vars.insert(name.clone(), value.clone());
            }
            "suite" => {
                dynamic_context
                    .suite_vars
                    .insert(name.clone(), value.clone());
            }
            "case" => {
                dynamic_context
                    .case_vars
                    .insert(name.clone(), value.clone());
            }
            "iteration" => {
                dynamic_context
                    .iteration_vars
                    .insert(name.clone(), value.clone());
            }
            other => {
                return Err(format!(
                    "Variable scope '{}' cannot define custom variables",
                    other
                ))
            }
        }
        resolved.insert(name.clone(), value);
    }
    Ok(resolved)
}

fn validate_variable_type(kind: &str, value: &Value) -> Result<(), String> {
    let valid = match kind {
        "string" | "datetime" => value.is_string(),
        "integer" => value.as_i64().is_some(),
        "decimal" => value.as_f64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "SecretRef" => false,
        _ => return Err(format!("Unsupported variable type '{}'", kind)),
    };
    if valid {
        Ok(())
    } else {
        Err(format!("value does not match type '{}'", kind))
    }
}

pub fn validate_variable_definitions(definitions: Option<&Value>) -> Result<(), String> {
    let Some(definitions) = definitions else {
        return Ok(());
    };
    let object = definitions
        .as_object()
        .ok_or_else(|| "Variable definitions must be a JSON object".to_string())?;
    if object.len() > 100
        || serde_json::to_vec(definitions).map_or(true, |bytes| bytes.len() > 65_536)
    {
        return Err("Variable definitions exceed the configured count or size limit".to_string());
    }
    let allowed_functions = [
        "fn.uuid_v4",
        "fn.random_int",
        "fn.random_string",
        "fn.synthetic_email",
        "fn.concat",
        "fn.lower",
        "fn.upper",
        "fn.trim",
        "fn.replace",
        "fn.length",
        "fn.add",
        "fn.subtract",
        "fn.round",
        "fn.date_format",
        "fn.date_add",
        "fn.json_extract",
        "fn.url_encode",
    ];
    for (name, definition) in object {
        let mut chars = name.chars();
        let valid_start = chars
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic());
        if name.len() > 128
            || !valid_start
            || chars.any(|ch| ch != '_' && !ch.is_ascii_alphanumeric())
        {
            return Err(format!("Variable name '{}' is invalid", name));
        }
        if let Some(function) = definition.get("function").and_then(Value::as_str) {
            if !allowed_functions.contains(&function) {
                return Err(format!(
                    "Variable '{}' uses an unsupported built-in function",
                    name
                ));
            }
            if definition
                .get("args")
                .and_then(Value::as_array)
                .is_none_or(|args| args.len() > 10)
            {
                return Err(format!(
                    "Variable '{}' requires at most 10 function arguments",
                    name
                ));
            }
        } else if let (Some(kind), Some(value)) = (
            definition.get("type").and_then(Value::as_str),
            definition.get("value"),
        ) {
            validate_variable_type(kind, value)
                .map_err(|error| format!("Variable '{}': {}", name, error))?;
        }
    }
    Ok(())
}

impl<'a> VariableResolver<'a> {
    pub fn new(context: &'a ResolutionContext) -> Self {
        let rng = StdRng::seed_from_u64(context.seed);
        Self { context, rng }
    }

    /// Resolve a reference by scope and path.
    /// Example: scope="env", path="base_url" or scope="step", path="login.token"
    pub fn resolve_ref(&mut self, scope: &str, path: &str) -> Result<Value, String> {
        match scope {
            "sys" => match path {
                "run_id" => Ok(Value::String(self.context.run_id.clone())),
                "suite_id" => Ok(Value::String(self.context.suite_id.clone())),
                "case_id" => Ok(Value::String(self.context.case_id.clone())),
                "iteration_id" => Ok(Value::String(self.context.iteration_id.clone())),
                "started_at_utc" => Ok(Value::String(self.context.started_at_utc.clone())),
                other => Err(format!("Unknown sys variable: {}", other)),
            },
            "env" => self
                .context
                .env_vars
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Environment variable '{}' not found", path)),
            "run" => self
                .context
                .run_vars
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Run input '{}' not found", path)),
            "suite" => self
                .context
                .suite_vars
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Suite variable '{}' not found", path)),
            "case" => self
                .context
                .case_vars
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Case variable '{}' not found", path)),
            "iteration" => self
                .context
                .iteration_vars
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Iteration data variable '{}' not found", path)),
            "step" => {
                // path format: "{node_name_or_id}.output.{key}" or "{node_name_or_id}.{key}"
                let parts: Vec<&str> = path.split('.').collect();
                if parts.len() < 2 {
                    return Err(format!(
                        "Invalid step path '{}', expected 'step_id.key'",
                        path
                    ));
                }
                let step_name = parts[0];
                let output_key = if parts.len() >= 3 && parts[1] == "output" {
                    parts[2]
                } else {
                    parts[1]
                };

                let step_map = self
                    .context
                    .step_outputs
                    .get(step_name)
                    .ok_or_else(|| format!("Step '{}' outputs not found", step_name))?;

                step_map.get(output_key).cloned().ok_or_else(|| {
                    format!("Output '{}' not found on step '{}'", output_key, step_name)
                })
            }
            "secret" => {
                let masked = self
                    .context
                    .secrets
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| format!("[SECRET:{}]", path));
                Ok(Value::String(masked))
            }
            other => Err(format!("Unknown scope '{}'", other)),
        }
    }

    /// Evaluate an allow-listed built-in function
    pub fn evaluate_function(
        &mut self,
        function_id: &str,
        args: &[Value],
    ) -> Result<Value, String> {
        match function_id {
            "fn.uuid_v4" => {
                let mut bytes = [0u8; 16];
                self.rng.fill_bytes(&mut bytes);
                bytes[6] = (bytes[6] & 0x0f) | 0x40;
                bytes[8] = (bytes[8] & 0x3f) | 0x80;
                Ok(Value::String(uuid::Uuid::from_bytes(bytes).to_string()))
            }
            "fn.random_int" => {
                let min = args
                    .get(0)
                    .and_then(|v| v.as_i64())
                    .ok_or("fn.random_int requires integer min")?;
                let max = args
                    .get(1)
                    .and_then(|v| v.as_i64())
                    .ok_or("fn.random_int requires integer max")?;
                if min > max {
                    return Err("min cannot be greater than max".to_string());
                }
                let val = self.rng.gen_range(min..=max);
                Ok(json!(val))
            }
            "fn.random_string" => {
                let len = args.get(0).and_then(|v| v.as_u64()).unwrap_or(8);
                if len > 128 {
                    return Err("fn.random_string length is bounded to 128".to_string());
                }
                let len = len as usize;
                let charset = args
                    .get(1)
                    .and_then(|v| v.as_str())
                    .unwrap_or("abcdefghijklmnopqrstuvwxyz0123456789");
                let chars: Vec<char> = charset.chars().collect();
                if chars.is_empty() || chars.len() > 128 {
                    return Err("charset must contain 1 to 128 characters".to_string());
                }
                let s: String = (0..len)
                    .map(|_| {
                        let idx = self.rng.gen_range(0..chars.len());
                        chars[idx]
                    })
                    .collect();
                Ok(Value::String(s))
            }
            "fn.synthetic_email" => {
                let prefix = args.get(0).and_then(|v| v.as_str()).unwrap_or("testuser");
                let domain = args
                    .get(1)
                    .and_then(|v| v.as_str())
                    .unwrap_or("example.test");
                if prefix.len() > 64 || domain.len() > 128 {
                    return Err("fn.synthetic_email input is too long".to_string());
                }
                if ![".test", ".example", ".invalid", ".localhost"]
                    .iter()
                    .any(|suffix| domain.ends_with(suffix))
                {
                    return Err(
                        "fn.synthetic_email is restricted to reserved test domains".to_string()
                    );
                }
                let rand_suffix: u32 = self.rng.gen_range(1000..9999);
                Ok(Value::String(format!(
                    "{}-{}@{}",
                    prefix, rand_suffix, domain
                )))
            }
            "fn.concat" => {
                let mut out = String::new();
                for a in args {
                    if let Some(s) = a.as_str() {
                        out.push_str(s);
                    } else {
                        out.push_str(&a.to_string());
                    }
                }
                if out.len() > 4096 {
                    return Err("fn.concat output exceeds 4096 characters".to_string());
                }
                Ok(Value::String(out))
            }
            "fn.lower" => {
                let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                Ok(Value::String(s.to_lowercase()))
            }
            "fn.upper" => {
                let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                Ok(Value::String(s.to_uppercase()))
            }
            "fn.trim" => {
                let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                Ok(Value::String(s.trim().to_string()))
            }
            "fn.replace" => {
                let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let pat = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                let rep = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
                if pat.len() > 256 || rep.len() > 4096 || s.len() > 65_536 {
                    return Err("fn.replace inputs exceed the configured size limit".to_string());
                }
                Ok(Value::String(s.replace(pat, rep)))
            }
            "fn.length" => {
                let len = match args.get(0) {
                    Some(Value::String(s)) => s.len(),
                    Some(Value::Array(arr)) => arr.len(),
                    Some(Value::Object(map)) => map.len(),
                    _ => 0,
                };
                Ok(json!(len))
            }
            "fn.add" => {
                let a = args
                    .get(0)
                    .and_then(|v| v.as_f64())
                    .ok_or("fn.add requires two numbers")?;
                let b = args
                    .get(1)
                    .and_then(|v| v.as_f64())
                    .ok_or("fn.add requires two numbers")?;
                Ok(json!(a + b))
            }
            "fn.subtract" => {
                let a = args
                    .get(0)
                    .and_then(|v| v.as_f64())
                    .ok_or("fn.subtract requires two numbers")?;
                let b = args
                    .get(1)
                    .and_then(|v| v.as_f64())
                    .ok_or("fn.subtract requires two numbers")?;
                Ok(json!(a - b))
            }
            "fn.round" => {
                let val = args
                    .get(0)
                    .and_then(|v| v.as_f64())
                    .ok_or("fn.round requires a number")?;
                let decimals = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                if !(-6..=6).contains(&decimals) {
                    return Err("fn.round decimal places are bounded to -6 through 6".to_string());
                }
                let decimals = decimals as i32;
                let factor = 10f64.powi(decimals);
                Ok(json!((val * factor).round() / factor))
            }
            "fn.date_format" => {
                let date_str = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let format = args.get(1).and_then(|v| v.as_str()).unwrap_or("%Y-%m-%d");
                if format.len() > 128 {
                    return Err("fn.date_format format is bounded to 128 characters".to_string());
                }
                let dt = DateTime::parse_from_rfc3339(date_str)
                    .map_err(|_| "fn.date_format requires an ISO-8601 timestamp".to_string())?
                    .with_timezone(&Utc);
                Ok(Value::String(dt.format(format).to_string()))
            }
            "fn.date_add" => {
                let date_str = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let amount = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                let unit = args.get(2).and_then(|v| v.as_str()).unwrap_or("days");
                if amount.unsigned_abs() > 36_500 {
                    return Err("fn.date_add is bounded to 36500 units".to_string());
                }
                let dt = DateTime::parse_from_rfc3339(date_str)
                    .map_err(|_| "fn.date_add requires an ISO-8601 timestamp".to_string())?
                    .with_timezone(&Utc);
                let result = match unit {
                    "seconds" => dt + Duration::seconds(amount),
                    "minutes" => dt + Duration::minutes(amount),
                    "hours" => dt + Duration::hours(amount),
                    "days" => dt + Duration::days(amount),
                    _ => {
                        return Err(
                            "fn.date_add unit must be seconds, minutes, hours, or days".to_string()
                        )
                    }
                };
                Ok(Value::String(result.to_rfc3339()))
            }
            "fn.json_extract" => {
                let target = args.get(0).unwrap_or(&Value::Null);
                let path = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                Ok(extract_json_path(target, path))
            }
            "fn.url_encode" => {
                let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let encoded: String = s
                    .as_bytes()
                    .iter()
                    .map(|byte| {
                        if byte.is_ascii_alphanumeric()
                            || matches!(*byte, b'-' | b'_' | b'.' | b'~')
                        {
                            (*byte as char).to_string()
                        } else {
                            format!("%{:02X}", byte)
                        }
                    })
                    .collect();
                Ok(Value::String(encoded))
            }
            other => Err(format!("Unknown function id '{}'", other)),
        }
    }

    /// Interpolate template expressions like `{{env.api_base_url}}/users/{{iteration.id}}`
    pub fn interpolate_string(&mut self, text: &str) -> Result<String, String> {
        let re = Regex::new(r"\{\{([a-zA-Z_]+)\.([a-zA-Z0-9_\.]+)\}\}").unwrap();
        let mut result = text.to_string();

        for cap in re.captures_iter(text) {
            let full_match = &cap[0];
            let scope = &cap[1];
            let path = &cap[2];

            let resolved_val = self.resolve_ref(scope, path)?;
            let replacement = match resolved_val {
                Value::String(s) => s,
                Value::Null => "".to_string(),
                other => other.to_string(),
            };
            result = result.replace(full_match, &replacement);
        }

        Ok(result)
    }

    /// Recursively resolve JSON value (objects, arrays, strings)
    pub fn resolve_json_value(&mut self, val: &Value) -> Result<Value, String> {
        match val {
            Value::String(s) => {
                // If it's a standalone single token like `{{env.num}}`, preserve its underlying type
                let re_exact = Regex::new(r"^\{\{([a-zA-Z_]+)\.([a-zA-Z0-9_\.]+)\}\}$").unwrap();
                if let Some(cap) = re_exact.captures(s) {
                    let scope = &cap[1];
                    let path = &cap[2];
                    return self.resolve_ref(scope, path);
                }
                // Otherwise string interpolation
                let interpolated = self.interpolate_string(s)?;
                Ok(Value::String(interpolated))
            }
            Value::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    out.push(self.resolve_json_value(item)?);
                }
                Ok(Value::Array(out))
            }
            Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    out.insert(k.clone(), self.resolve_json_value(v)?);
                }
                Ok(Value::Object(out))
            }
            primitive => Ok(primitive.clone()),
        }
    }
}

fn extract_json_path(val: &Value, path: &str) -> Value {
    if path.is_empty() || path == "$" {
        return val.clone();
    }
    let clean_path = path.trim_start_matches("$.");
    let parts: Vec<&str> = clean_path.split('.').collect();

    let mut curr = val;
    for part in parts {
        // Handle array index like items[0]
        if let Some(idx_start) = part.find('[') {
            if let Some(idx_end) = part.find(']') {
                let key = &part[..idx_start];
                let idx_str = &part[idx_start + 1..idx_end];
                if !key.is_empty() {
                    curr = match curr.get(key) {
                        Some(v) => v,
                        None => return Value::Null,
                    };
                }
                if let Ok(idx) = idx_str.parse::<usize>() {
                    curr = match curr.get(idx) {
                        Some(v) => v,
                        None => return Value::Null,
                    };
                }
                continue;
            }
        }

        curr = match curr.get(part) {
            Some(v) => v,
            None => return Value::Null,
        };
    }

    curr.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_interpolation() {
        let mut ctx = ResolutionContext::default();
        ctx.env_vars
            .insert("base_url".to_string(), json!("https://api.test.com"));
        ctx.iteration_vars.insert("user_id".to_string(), json!(104));

        let mut resolver = VariableResolver::new(&ctx);
        let template = "{{env.base_url}}/users/{{iteration.user_id}}";
        let resolved = resolver
            .interpolate_string(template)
            .expect("interpolation");
        assert_eq!(resolved, "https://api.test.com/users/104");
    }

    #[test]
    fn test_built_in_functions() {
        let ctx = ResolutionContext::default();
        let mut resolver = VariableResolver::new(&ctx);

        let res = resolver
            .evaluate_function("fn.concat", &[json!("hello "), json!("world")])
            .unwrap();
        assert_eq!(res, json!("hello world"));

        let res = resolver
            .evaluate_function("fn.add", &[json!(10), json!(25)])
            .unwrap();
        assert_eq!(res, json!(35.0));

        let res = resolver
            .evaluate_function("fn.lower", &[json!("HeLLo")])
            .unwrap();
        assert_eq!(res, json!("hello"));
    }

    #[test]
    fn test_json_extract() {
        let val = json!({
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]
        });

        assert_eq!(extract_json_path(&val, "users[1].name"), json!("Bob"));
    }
}
