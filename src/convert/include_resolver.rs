use std::collections::{BTreeMap, HashSet};

use crate::domain::compose::{ComposeProject, parse_compose_documents};
use crate::domain::git_fetch::RepoRef;

/// Context for resolving include paths.
#[derive(Debug, Clone)]
pub enum IncludeContext {
    /// Resolve relative paths against a Git repository.
    Git(RepoRef),
    /// No remote context; resolve only against the local file registry.
    Local,
}

/// Result of resolving all includes for a set of Compose documents.
pub struct IncludeResolution {
    /// All projects in merge order: included projects first (depth-first),
    /// then the original documents in order.
    pub projects: Vec<ComposeProject>,
    /// Messages about unresolved includes and other issues.
    pub diagnostics: Vec<String>,
    /// Paths that need to be fetched from Git (only populated for Git context).
    /// The caller is responsible for fetching these and re-calling the resolver.
    pub pending_fetches: Vec<PendingFetch>,
}

/// A file that needs to be fetched before resolution can complete.
#[derive(Debug, Clone)]
pub struct PendingFetch {
    pub path: String,
    pub raw_url: String,
}

/// Resolve all includes from a list of parsed Compose documents.
///
/// `file_registry` maps known file paths to their YAML contents.
/// When a Git context is provided and an include path is missing from the
/// registry, a `PendingFetch` entry is returned so the caller can fetch
/// the file and retry.
pub fn resolve_includes(
    documents: Vec<ComposeProject>,
    context: &IncludeContext,
    file_registry: &BTreeMap<String, String>,
) -> IncludeResolution {
    let mut all_projects = Vec::new();
    let mut diagnostics = Vec::new();
    let mut pending_fetches = Vec::new();
    let mut visited = HashSet::new();

    for doc in &documents {
        resolve_recursive(
            doc,
            context,
            file_registry,
            &mut visited,
            &mut all_projects,
            &mut diagnostics,
            &mut pending_fetches,
        );
    }

    // The original documents come last (higher precedence).
    for doc in documents {
        all_projects.push(doc);
    }

    IncludeResolution {
        projects: all_projects,
        diagnostics,
        pending_fetches,
    }
}

fn resolve_recursive(
    project: &ComposeProject,
    context: &IncludeContext,
    file_registry: &BTreeMap<String, String>,
    visited: &mut HashSet<String>,
    resolved: &mut Vec<ComposeProject>,
    diagnostics: &mut Vec<String>,
    pending_fetches: &mut Vec<PendingFetch>,
) {
    for include in &project.includes {
        for path in &include.paths {
            let normalized = normalize_path(path);

            if !visited.insert(normalized.clone()) {
                diagnostics.push(format!(
                    "Include cycle detected for `{normalized}`; skipping."
                ));
                continue;
            }

            if let Some(content) = file_registry.get(&normalized) {
                match parse_compose_documents(content) {
                    Ok(sub_docs) => {
                        for sub_doc in &sub_docs {
                            // Recurse into included files' own includes.
                            resolve_recursive(
                                sub_doc,
                                context,
                                file_registry,
                                visited,
                                resolved,
                                diagnostics,
                                pending_fetches,
                            );
                        }
                        resolved.extend(sub_docs);
                    }
                    Err(err) => {
                        diagnostics
                            .push(format!("Failed to parse included file `{normalized}`: {err}"));
                    }
                }
            } else {
                match context {
                    IncludeContext::Git(repo_ref) => {
                        let include_ref = resolve_git_path(repo_ref, &normalized);
                        let raw_url =
                            crate::domain::git_fetch::raw_content_url(&include_ref);
                        pending_fetches.push(PendingFetch {
                            path: normalized.clone(),
                            raw_url,
                        });
                        diagnostics.push(format!(
                            "Include `{normalized}` will be fetched from Git."
                        ));
                    }
                    IncludeContext::Local => {
                        diagnostics.push(format!(
                            "Include `{normalized}` not found. Upload or paste the file to resolve it."
                        ));
                    }
                }
            }
        }
    }
}

/// Resolve an include path relative to the base path of a RepoRef.
fn resolve_git_path(base: &RepoRef, include_path: &str) -> RepoRef {
    let base_dir = match base.path.rfind('/') {
        Some(idx) => &base.path[..idx],
        None => "",
    };

    let resolved = if include_path.starts_with('/') {
        include_path.trim_start_matches('/').to_string()
    } else if base_dir.is_empty() {
        include_path.to_string()
    } else {
        format!("{base_dir}/{include_path}")
    };

    // Simplify ././../  segments
    let simplified = simplify_path(&resolved);

    RepoRef {
        provider: base.provider.clone(),
        owner: base.owner.clone(),
        repo: base.repo.clone(),
        git_ref: base.git_ref.clone(),
        path: simplified,
    }
}

/// Normalize an include path by stripping leading `./` and collapsing redundant separators.
fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    let without_prefix = trimmed.strip_prefix("./").unwrap_or(trimmed);
    without_prefix.to_string()
}

/// Simplify a path by resolving `.` and `..` segments.
fn simplify_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::git_fetch::{GitProvider, RepoRef};

    #[test]
    fn normalize_strips_leading_dot_slash() {
        assert_eq!(normalize_path("./docker-compose.yml"), "docker-compose.yml");
        assert_eq!(normalize_path("subdir/file.yml"), "subdir/file.yml");
    }

    #[test]
    fn simplify_resolves_parent_segments() {
        assert_eq!(simplify_path("a/b/../c"), "a/c");
        assert_eq!(simplify_path("./a/./b"), "a/b");
        assert_eq!(simplify_path("a/b/c/../../d"), "a/d");
    }

    #[test]
    fn resolve_git_path_relative_to_base_dir() {
        let base = RepoRef {
            provider: GitProvider::GitHub,
            owner: String::from("acme"),
            repo: String::from("app"),
            git_ref: String::from("main"),
            path: String::from("deploy/docker-compose.yml"),
        };

        let resolved = resolve_git_path(&base, "./other.yml");
        assert_eq!(resolved.path, "deploy/other.yml");

        let resolved2 = resolve_git_path(&base, "../shared/base.yml");
        assert_eq!(resolved2.path, "shared/base.yml");
    }

    #[test]
    fn resolve_git_path_with_root_base() {
        let base = RepoRef {
            provider: GitProvider::GitHub,
            owner: String::from("acme"),
            repo: String::from("app"),
            git_ref: String::from("main"),
            path: String::from("docker-compose.yml"),
        };

        let resolved = resolve_git_path(&base, "subdir/other.yml");
        assert_eq!(resolved.path, "subdir/other.yml");
    }

    #[test]
    fn local_context_reports_unresolved() {
        let project = ComposeProject {
            includes: vec![crate::domain::compose::ComposeInclude {
                paths: vec![String::from("missing.yml")],
                project_directory: None,
                env_files: Vec::new(),
            }],
            ..Default::default()
        };

        let result = resolve_includes(vec![project], &IncludeContext::Local, &BTreeMap::new());
        assert!(result.diagnostics.iter().any(|d| d.contains("missing.yml") && d.contains("not found")));
        assert!(result.pending_fetches.is_empty());
    }

    #[test]
    fn git_context_produces_pending_fetches() {
        let project = ComposeProject {
            includes: vec![crate::domain::compose::ComposeInclude {
                paths: vec![String::from("other/compose.yml")],
                project_directory: None,
                env_files: Vec::new(),
            }],
            ..Default::default()
        };

        let context = IncludeContext::Git(RepoRef {
            provider: GitProvider::GitHub,
            owner: String::from("acme"),
            repo: String::from("app"),
            git_ref: String::from("main"),
            path: String::from("docker-compose.yml"),
        });

        let result = resolve_includes(vec![project], &context, &BTreeMap::new());
        assert_eq!(result.pending_fetches.len(), 1);
        assert!(result.pending_fetches[0].raw_url.contains("raw.githubusercontent.com"));
        assert!(result.pending_fetches[0].raw_url.contains("other/compose.yml"));
    }

    #[test]
    fn resolved_includes_precede_main_documents() {
        let included_yaml = r#"
services:
  db:
    image: postgres:16
"#;

        let main = ComposeProject {
            name: Some(String::from("main")),
            includes: vec![crate::domain::compose::ComposeInclude {
                paths: vec![String::from("db.yml")],
                project_directory: None,
                env_files: Vec::new(),
            }],
            ..Default::default()
        };

        let mut registry = BTreeMap::new();
        registry.insert(String::from("db.yml"), included_yaml.to_string());

        let result = resolve_includes(vec![main], &IncludeContext::Local, &registry);
        assert!(result.diagnostics.is_empty());
        assert!(result.pending_fetches.is_empty());
        // First project is the included one (db), last is the main document.
        assert!(result.projects[0].services.contains_key("db"));
        assert_eq!(result.projects.last().unwrap().name.as_deref(), Some("main"));
    }

    #[test]
    fn cycle_detection_prevents_infinite_loop() {
        // a.yml includes b.yml, b.yml includes a.yml
        let a_yaml = r#"
include:
  - b.yml
services:
  web:
    image: nginx
"#;
        let b_yaml = r#"
include:
  - a.yml
services:
  api:
    image: node
"#;
        let main = ComposeProject {
            includes: vec![crate::domain::compose::ComposeInclude {
                paths: vec![String::from("a.yml")],
                project_directory: None,
                env_files: Vec::new(),
            }],
            ..Default::default()
        };

        let mut registry = BTreeMap::new();
        registry.insert(String::from("a.yml"), a_yaml.to_string());
        registry.insert(String::from("b.yml"), b_yaml.to_string());

        let result = resolve_includes(vec![main], &IncludeContext::Local, &registry);
        assert!(result.diagnostics.iter().any(|d| d.contains("cycle")));
        // Should still resolve what it can without hanging.
        assert!(result.projects.iter().any(|p| p.services.contains_key("web")));
        assert!(result.projects.iter().any(|p| p.services.contains_key("api")));
    }
}
