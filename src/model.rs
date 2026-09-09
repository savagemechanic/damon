use std::{cell::Cell, env, path::Path, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Ollama,
    External,
    Cloud,
}
#[derive(Debug, Clone)]
pub struct RoutedResponse {
    pub provider: ProviderKind,
    pub text: String,
}
#[derive(Debug, Clone)]
pub struct ModelRouter {
    pub ollama_model: String,
    pub external_command: Option<String>,
    pub cloud_command: Option<String>,
    pub allow_cloud: bool,
    pub cloud_call_limit: u32,
    cloud_calls: Cell<u32>,
}
impl Default for ModelRouter {
    fn default() -> Self {
        Self {
            ollama_model: env::var("DAMON_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:8b".into()),
            external_command: env::var("DAMON_MODEL_COMMAND").ok(),
            cloud_command: env::var("DAMON_CLOUD_COMMAND").ok(),
            allow_cloud: env::var("DAMON_ALLOW_CLOUD")
                .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
            cloud_call_limit: env::var("DAMON_CLOUD_CALL_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            cloud_calls: Cell::new(0),
        }
    }
}
impl ModelRouter {
    pub fn infer(&self, prompt: &str) -> Result<RoutedResponse, String> {
        self.infer_validated(prompt, |_| Ok(()))
    }
    pub fn infer_validated(
        &self,
        prompt: &str,
        validate: impl Fn(&str) -> Result<(), String>,
    ) -> Result<RoutedResponse, String> {
        if prompt.len() > 64 * 1024 {
            return Err("teacher context exceeds 64 KiB".into());
        }
        self.route(validate, |provider| {
            let result = match provider {
                ProviderKind::Ollama => crate::process::run(
                    Path::new("."),
                    "ollama",
                    &["run", &self.ollama_model],
                    Some(prompt),
                    Duration::from_secs(30),
                ),
                ProviderKind::External | ProviderKind::Cloud => {
                    let command = if provider == ProviderKind::External {
                        &self.external_command
                    } else {
                        &self.cloud_command
                    };
                    crate::process::run(
                        Path::new("."),
                        "sh",
                        &[
                            "-lc",
                            command.as_deref().ok_or("provider is not configured")?,
                        ],
                        Some(prompt),
                        Duration::from_secs(30),
                    )
                }
            };
            if result.success {
                Ok(result.stdout)
            } else {
                Err(format!(
                    "provider failed (exit {:?}): {}",
                    result.code,
                    result.stderr.trim()
                ))
            }
        })
    }
    fn route(
        &self,
        validate: impl Fn(&str) -> Result<(), String>,
        mut run: impl FnMut(ProviderKind) -> Result<String, String>,
    ) -> Result<RoutedResponse, String> {
        let mut failures = Vec::new();
        for provider in [
            ProviderKind::Ollama,
            ProviderKind::External,
            ProviderKind::Cloud,
        ] {
            let enabled = match provider {
                ProviderKind::Ollama => !self.ollama_model.is_empty(),
                ProviderKind::External => self.external_command.is_some(),
                ProviderKind::Cloud => {
                    self.allow_cloud
                        && self.cloud_command.is_some()
                        && self.cloud_calls.get() < self.cloud_call_limit
                }
            };
            if !enabled {
                continue;
            }
            // Count attempts, including failures. No automatic retry can spend beyond this limit.
            if provider == ProviderKind::Cloud {
                self.cloud_calls.set(self.cloud_calls.get() + 1);
            }
            match run(provider).and_then(|text| {
                if text.trim().is_empty() {
                    return Err("empty provider response".into());
                }
                validate(&text)?;
                Ok(text)
            }) {
                Ok(text) => return Ok(RoutedResponse { provider, text }),
                Err(e) => failures.push(format!("{provider:?}: {e}")),
            }
        }
        Err(format!("No valid teacher response. {}. Cloud requires explicit enablement and an available call budget.",failures.join("; ")))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn router() -> ModelRouter {
        ModelRouter {
            ollama_model: "local".into(),
            external_command: Some("free".into()),
            cloud_command: Some("paid".into()),
            allow_cloud: false,
            cloud_call_limit: 1,
            cloud_calls: Cell::new(0),
        }
    }
    #[test]
    fn disabled_cloud_is_never_invoked() {
        let r = router();
        let mut calls = vec![];
        assert!(r
            .route(
                |_| Ok(()),
                |p| {
                    calls.push(p);
                    Err("offline".into())
                }
            )
            .is_err());
        assert_eq!(calls, vec![ProviderKind::Ollama, ProviderKind::External]);
    }
    #[test]
    fn invalid_local_answer_falls_through_to_free_provider() {
        let r = router();
        let mut calls = vec![];
        let answer = r
            .route(
                |s| {
                    if s == "valid" {
                        Ok(())
                    } else {
                        Err("invalid graph".into())
                    }
                },
                |p| {
                    calls.push(p);
                    Ok(if p == ProviderKind::Ollama {
                        "bad"
                    } else {
                        "valid"
                    }
                    .into())
                },
            )
            .unwrap();
        assert_eq!(answer.provider, ProviderKind::External);
        assert_eq!(calls.len(), 2);
    }
    #[test]
    fn paid_attempts_have_a_session_limit_even_on_failure() {
        let mut r = router();
        r.allow_cloud = true;
        let mut paid = 0;
        for _ in 0..3 {
            let _ = r.route(
                |_| Ok(()),
                |p| {
                    if p == ProviderKind::Cloud {
                        paid += 1;
                    }
                    Err("offline".into())
                },
            );
        }
        assert_eq!(paid, 1);
    }
    #[test]
    fn local_success_stops_routing() {
        let r = router();
        let mut calls = 0;
        assert_eq!(
            r.route(
                |_| Ok(()),
                |_| {
                    calls += 1;
                    Ok("valid".into())
                }
            )
            .unwrap()
            .provider,
            ProviderKind::Ollama
        );
        assert_eq!(calls, 1);
    }
}
