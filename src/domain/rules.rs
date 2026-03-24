use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSet {
    pub registry_cache: Option<RegistryCacheRule>,
    #[serde(default)]
    pub env_translations: Vec<EnvTranslationRule>,
    pub base_ide_container: Option<IdeContainerRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCacheRule {
    pub prefix: String,
    #[serde(default)]
    pub mode: RegistryCacheMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistryCacheMode {
    #[default]
    Prepend,
    Replace,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvTranslationRule {
    #[serde(default = "default_service_match")]
    pub service: String,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default)]
    pub remove: bool,
    #[serde(default)]
    pub set: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeContainerRule {
    #[serde(default = "default_ide_name")]
    pub name: String,
    pub image: String,
    pub memory_limit: Option<String>,
}

fn default_ide_name() -> String {
    String::from("tool")
}

fn default_service_match() -> String {
    String::from("*")
}

pub fn load_default_rules() -> Result<RuleSet, String> {
    load_rules_from_json(include_str!("../../assets/rules/default-rules.json"))
}

pub fn load_rules_from_json(input: &str) -> Result<RuleSet, String> {
    serde_json::from_str::<RuleSet>(input).map_err(|err| err.to_string())
}

pub fn merge_rules(base: &RuleSet, extra: &RuleSet) -> RuleSet {
    let mut merged = base.clone();

    if extra.registry_cache.is_some() {
        merged.registry_cache = extra.registry_cache.clone();
    }

    if !extra.env_translations.is_empty() {
        merged
            .env_translations
            .extend(extra.env_translations.clone());
    }

    if extra.base_ide_container.is_some() {
        merged.base_ide_container = extra.base_ide_container.clone();
    }

    merged
}
