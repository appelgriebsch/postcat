//! Core data types, persistence, and `{{var}}` substitution.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    /// Fixed-width (4 char) label for list rows.
    pub fn short(self) -> &'static str {
        match self {
            Method::Get => "GET ",
            Method::Post => "POST",
            Method::Put => "PUT ",
            Method::Patch => "PTCH",
            Method::Delete => "DEL ",
            Method::Head => "HEAD",
            Method::Options => "OPTS",
        }
    }

    pub fn next(self) -> Method {
        let i = Method::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Method::ALL[(i + 1) % Method::ALL.len()]
    }

    pub fn prev(self) -> Method {
        let i = Method::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Method::ALL[(i + Method::ALL.len() - 1) % Method::ALL.len()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KV {
    pub key: String,
    pub value: String,
    pub enabled: bool,
}

impl Default for KV {
    fn default() -> Self {
        KV { key: String::new(), value: String::new(), enabled: true }
    }
}

impl KV {
    pub fn is_blank(&self) -> bool {
        self.key.is_empty() && self.value.is_empty()
    }
}

/// Count of rows that will actually be sent.
pub fn active_count(rows: &[KV]) -> usize {
    rows.iter().filter(|r| r.enabled && !r.key.is_empty()).count()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum BodyType {
    #[default]
    None,
    Json,
    Text,
    Form,
}

impl BodyType {
    pub fn as_str(self) -> &'static str {
        match self {
            BodyType::None => "none",
            BodyType::Json => "JSON",
            BodyType::Text => "text",
            BodyType::Form => "form",
        }
    }

    pub fn next(self) -> BodyType {
        match self {
            BodyType::None => BodyType::Json,
            BodyType::Json => BodyType::Text,
            BodyType::Text => BodyType::Form,
            BodyType::Form => BodyType::None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AuthType {
    #[default]
    None,
    Bearer,
    Basic,
}

impl AuthType {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthType::None => "none",
            AuthType::Bearer => "Bearer",
            AuthType::Basic => "Basic",
        }
    }

    pub fn next(self) -> AuthType {
        match self {
            AuthType::None => AuthType::Bearer,
            AuthType::Bearer => AuthType::Basic,
            AuthType::Basic => AuthType::None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Auth {
    #[serde(default)]
    pub typ: AuthType,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub pass: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestModel {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: Method,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub params: Vec<KV>,
    #[serde(default)]
    pub headers: Vec<KV>,
    #[serde(default)]
    pub body_type: BodyType,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub form: Vec<KV>,
    #[serde(default)]
    pub auth: Auth,
}

impl Default for RequestModel {
    fn default() -> Self {
        RequestModel {
            name: String::new(),
            method: Method::Get,
            url: String::new(),
            params: vec![KV::default()],
            headers: vec![KV::default()],
            body_type: BodyType::None,
            body: String::new(),
            form: vec![KV::default()],
            auth: Auth::default(),
        }
    }
}

impl RequestModel {
    pub fn display_name(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else if !self.url.is_empty() {
            &self.url
        } else {
            "untitled"
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub request: RequestModel,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub elapsed_ms: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Persisted {
    #[serde(default)]
    pub saved: Vec<RequestModel>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub env: Vec<KV>,
    #[serde(default)]
    pub draft: Option<RequestModel>,
}

/// Where the workspace lives for a normal run.
pub fn default_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("postcat").join("state.json"))
}

impl Persisted {
    /// `None` means "no workspace file" — load yields defaults, save is a no-op.
    /// Tests use that to stay off the user's real config.
    pub fn load(path: Option<&Path>) -> Persisted {
        path.and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: Option<&Path>) {
        let Some(path) = path else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }
}

/// Replace `{{name}}` (and `{{ name }}`) with values from enabled env vars.
pub fn substitute(input: &str, env: &[KV]) -> String {
    if !input.contains("{{") {
        return input.to_string();
    }
    let mut out = input.to_string();
    for kv in env.iter().filter(|k| k.enabled && !k.key.is_empty()) {
        out = out.replace(&format!("{{{{{}}}}}", kv.key), &kv.value);
        out = out.replace(&format!("{{{{ {} }}}}", kv.key), &kv.value);
    }
    out
}

pub fn human_size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<KV> {
        pairs
            .iter()
            .map(|(k, v)| KV { key: k.to_string(), value: v.to_string(), enabled: true })
            .collect()
    }

    #[test]
    fn substitutes_vars_with_and_without_spaces() {
        let e = env(&[("base", "https://api.dev"), ("id", "42")]);
        assert_eq!(substitute("{{base}}/u/{{id}}", &e), "https://api.dev/u/42");
        assert_eq!(substitute("{{ base }}/x", &e), "https://api.dev/x");
    }

    #[test]
    fn leaves_unknown_and_disabled_vars_alone() {
        let mut e = env(&[("known", "yes")]);
        e.push(KV { key: "off".into(), value: "no".into(), enabled: false });
        assert_eq!(substitute("{{unknown}}", &e), "{{unknown}}");
        assert_eq!(substitute("{{off}}", &e), "{{off}}");
        assert_eq!(substitute("plain text", &e), "plain text");
    }

    #[test]
    fn counts_only_rows_that_will_be_sent() {
        let rows = vec![
            KV { key: "a".into(), value: "1".into(), enabled: true },
            KV { key: "b".into(), value: "2".into(), enabled: false },
            KV { key: String::new(), value: "orphan".into(), enabled: true },
        ];
        assert_eq!(active_count(&rows), 1);
    }

    #[test]
    fn methods_cycle_both_ways_and_wrap() {
        assert_eq!(Method::Get.next(), Method::Post);
        assert_eq!(Method::Get.prev(), Method::Options, "wraps backwards");
        assert_eq!(Method::Options.next(), Method::Get, "wraps forwards");
        for m in Method::ALL {
            assert_eq!(m.next().prev(), m, "{m:?} round-trips");
            assert_eq!(m.short().chars().count(), 4, "{m:?} label is padded");
        }
    }

    #[test]
    fn body_and_auth_types_cycle_back_to_the_start() {
        assert_eq!(BodyType::None.next().next().next().next(), BodyType::None);
        assert_eq!(AuthType::None.next().next().next(), AuthType::None);
    }

    #[test]
    fn display_name_falls_back_from_name_to_url() {
        let mut r = RequestModel::default();
        assert_eq!(r.display_name(), "untitled");
        r.url = "http://x.dev".into();
        assert_eq!(r.display_name(), "http://x.dev");
        r.name = "named".into();
        assert_eq!(r.display_name(), "named");
    }

    #[test]
    fn human_size_scales_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn a_missing_workspace_file_loads_defaults_and_never_panics() {
        let missing = std::path::Path::new("/nonexistent/postcat/state.json");
        let loaded = Persisted::load(Some(missing));
        assert!(loaded.saved.is_empty() && loaded.history.is_empty());
        // No path at all: load is empty, save is a silent no-op.
        assert!(Persisted::load(None).saved.is_empty());
        Persisted::default().save(None);
    }
}
