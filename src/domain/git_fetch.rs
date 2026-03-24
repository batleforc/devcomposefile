/// Translates user-friendly Git hosting URLs to raw-content URLs
/// and provides the fetch logic for retrieving Compose files from repositories.

/// Default file paths to try in order when no explicit path is provided.
pub const DEFAULT_COMPOSE_PATHS: &[&str] = &["docker-compose.yml", "compose.yaml"];

/// Supported Git hosting providers.
#[derive(Debug, Clone, PartialEq)]
pub enum GitProvider {
    GitHub,
    GitLab,
    Bitbucket,
}

/// Parsed repository reference.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoRef {
    pub provider: GitProvider,
    pub owner: String,
    pub repo: String,
    pub git_ref: String,
    pub path: String,
}

/// Parse a user-provided URL + optional overrides into a `RepoRef`.
///
/// Accepts URLs like:
/// - `https://github.com/owner/repo`
/// - `https://gitlab.com/owner/repo`
/// - `https://bitbucket.org/owner/repo`
/// - `https://github.com/owner/repo/blob/main/docker-compose.yml`
///
/// `ref_override` overrides the branch/tag extracted from the URL.
/// `path_override` overrides the file path extracted from the URL.
pub fn parse_repo_url(
    url: &str,
    ref_override: Option<&str>,
    path_override: Option<&str>,
) -> Result<RepoRef, String> {
    let url = url.trim().trim_end_matches('/');

    // Strip scheme
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| String::from("URL must start with https:// or http://"))?;

    let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
    if parts.len() < 2 {
        return Err(String::from(
            "URL must include a repository path (e.g. github.com/owner/repo)",
        ));
    }

    let host = parts[0].to_lowercase();
    let remainder = parts[1];

    let provider = if host.contains("github.com") {
        GitProvider::GitHub
    } else if host.contains("gitlab.com") {
        GitProvider::GitLab
    } else if host.contains("bitbucket.org") {
        GitProvider::Bitbucket
    } else {
        return Err(format!(
            "Unsupported Git host `{host}`. Supported: github.com, gitlab.com, bitbucket.org"
        ));
    };

    let segments: Vec<&str> = remainder.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(String::from(
            "URL must include owner and repository (e.g. owner/repo)",
        ));
    }

    let owner = segments[0].to_string();
    let repo = segments[1].trim_end_matches(".git").to_string();

    // Try to extract ref and path from URL path segments
    // GitHub/GitLab: /owner/repo/blob/<ref>/<path...>
    // GitHub/GitLab: /owner/repo/tree/<ref>/<path...>
    // GitHub raw:    /owner/repo/raw/<ref>/<path...>
    // Bitbucket:     /owner/repo/src/<ref>/<path...>
    let (extracted_ref, extracted_path) = if segments.len() > 3 {
        let kind = segments[2]; // blob, tree, raw, src, -/blob, etc.
        let remaining = &segments[3..];

        match kind {
            "blob" | "tree" | "raw" | "src" => {
                let git_ref = remaining[0].to_string();
                let path = if remaining.len() > 1 {
                    remaining[1..].join("/")
                } else {
                    String::new()
                };
                (
                    Some(git_ref),
                    if path.is_empty() { None } else { Some(path) },
                )
            }
            // GitLab uses /-/blob/<ref>/<path>
            "-" if segments.len() > 4
                && (segments[3] == "blob" || segments[3] == "tree" || segments[3] == "raw") =>
            {
                let remaining = &segments[4..];
                if remaining.is_empty() {
                    (None, None)
                } else {
                    let git_ref = remaining[0].to_string();
                    let path = if remaining.len() > 1 {
                        remaining[1..].join("/")
                    } else {
                        String::new()
                    };
                    (
                        Some(git_ref),
                        if path.is_empty() { None } else { Some(path) },
                    )
                }
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    let git_ref = ref_override
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .or(extracted_ref)
        .unwrap_or_else(|| String::from("main"));

    let path = path_override
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().trim_start_matches('/').to_string())
        .or(extracted_path)
        .unwrap_or_else(|| String::from("docker-compose.yml"));

    Ok(RepoRef {
        provider,
        owner,
        repo,
        git_ref,
        path,
    })
}

/// Build the raw-content URL for fetching the file.
pub fn raw_content_url(repo_ref: &RepoRef) -> String {
    match repo_ref.provider {
        GitProvider::GitHub => {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                repo_ref.owner, repo_ref.repo, repo_ref.git_ref, repo_ref.path
            )
        }
        GitProvider::GitLab => {
            format!(
                "https://gitlab.com/{}/{}/-/raw/{}/{}",
                repo_ref.owner, repo_ref.repo, repo_ref.git_ref, repo_ref.path
            )
        }
        GitProvider::Bitbucket => {
            format!(
                "https://bitbucket.org/{}/{}/raw/{}/{}",
                repo_ref.owner, repo_ref.repo, repo_ref.git_ref, repo_ref.path
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_simple_url() {
        let r = parse_repo_url("https://github.com/acme/webapp", None, None).unwrap();
        assert_eq!(r.provider, GitProvider::GitHub);
        assert_eq!(r.owner, "acme");
        assert_eq!(r.repo, "webapp");
        assert_eq!(r.git_ref, "main");
        assert_eq!(r.path, "docker-compose.yml");
    }

    #[test]
    fn parse_github_blob_url() {
        let r = parse_repo_url(
            "https://github.com/acme/webapp/blob/develop/infra/compose.yml",
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.git_ref, "develop");
        assert_eq!(r.path, "infra/compose.yml");
    }

    #[test]
    fn parse_gitlab_dash_blob_url() {
        let r = parse_repo_url(
            "https://gitlab.com/team/project/-/blob/v2.0/docker-compose.yaml",
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.provider, GitProvider::GitLab);
        assert_eq!(r.git_ref, "v2.0");
        assert_eq!(r.path, "docker-compose.yaml");
    }

    #[test]
    fn parse_bitbucket_src_url() {
        let r = parse_repo_url(
            "https://bitbucket.org/team/repo/src/release/compose/app.yml",
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.provider, GitProvider::Bitbucket);
        assert_eq!(r.git_ref, "release");
        assert_eq!(r.path, "compose/app.yml");
    }

    #[test]
    fn overrides_take_precedence() {
        let r = parse_repo_url(
            "https://github.com/acme/webapp/blob/main/old.yml",
            Some("feature-branch"),
            Some("new/path.yml"),
        )
        .unwrap();
        assert_eq!(r.git_ref, "feature-branch");
        assert_eq!(r.path, "new/path.yml");
    }

    #[test]
    fn strips_dot_git_suffix() {
        let r = parse_repo_url("https://github.com/acme/webapp.git", None, None).unwrap();
        assert_eq!(r.repo, "webapp");
    }

    #[test]
    fn rejects_unsupported_host() {
        let r = parse_repo_url("https://sourcehut.org/acme/repo", None, None);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("Unsupported Git host"));
    }

    #[test]
    fn rejects_missing_scheme() {
        let r = parse_repo_url("github.com/acme/repo", None, None);
        assert!(r.is_err());
    }

    #[test]
    fn raw_content_url_github() {
        let r = RepoRef {
            provider: GitProvider::GitHub,
            owner: String::from("acme"),
            repo: String::from("app"),
            git_ref: String::from("main"),
            path: String::from("docker-compose.yml"),
        };
        assert_eq!(
            raw_content_url(&r),
            "https://raw.githubusercontent.com/acme/app/main/docker-compose.yml"
        );
    }

    #[test]
    fn raw_content_url_gitlab() {
        let r = RepoRef {
            provider: GitProvider::GitLab,
            owner: String::from("team"),
            repo: String::from("proj"),
            git_ref: String::from("dev"),
            path: String::from("compose.yml"),
        };
        assert_eq!(
            raw_content_url(&r),
            "https://gitlab.com/team/proj/-/raw/dev/compose.yml"
        );
    }

    #[test]
    fn raw_content_url_bitbucket() {
        let r = RepoRef {
            provider: GitProvider::Bitbucket,
            owner: String::from("org"),
            repo: String::from("svc"),
            git_ref: String::from("v1"),
            path: String::from("infra/docker-compose.yml"),
        };
        assert_eq!(
            raw_content_url(&r),
            "https://bitbucket.org/org/svc/raw/v1/infra/docker-compose.yml"
        );
    }
}
