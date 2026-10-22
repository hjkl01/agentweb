//! Thin wrapper over `pi list` / `pi install` / `pi remove` / `pi update`.
//!
//! Pi owns the package list (`settings.json`), the installed trees, and how packages
//! contribute skills/prompts/extensions, so AgentWeb never reads or writes those itself:
//! it displays what `pi list` reported and hands every mutation back to Pi.
//!
//! Project-scope entries are only listed and only mutated when Pi itself trusts the
//! directory (the same rule as skills); AgentWeb never passes `--approve` on the user's behalf.

use super::adapter::AgentConfig;
use super::models::run_agent_command;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Install and update fetch from npm/git; a plain list answers in well under a second.
const PLUGINS_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub source: String,
    /// `user` or `project`, as reported by Pi.
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub async fn list(config: &AgentConfig) -> Result<Vec<PackageInfo>> {
    let (success, stdout, stderr) =
        run_agent_command(config, &["list"], Some(PLUGINS_TIMEOUT)).await?;
    if !success {
        return Err(cli_error(&stdout, &stderr));
    }
    Ok(parse(&stdout))
}

/// `pi list` answers as a small text report:
///
/// ```text
/// User packages:
///   git:github.com/user/repo
///     /home/user/.pi/agent/git/github.com/user/repo
///
/// Project packages:
///   ./local/path
/// ```
///
/// A path line is optional (a configured package that is not installed has none), and an
/// empty store prints `No packages installed.`, so unknown lines are simply skipped.
fn parse(text: &str) -> Vec<PackageInfo> {
    let mut packages: Vec<PackageInfo> = Vec::new();
    let mut scope = "user";
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(heading) = trimmed.strip_suffix(" packages:") {
            scope = if heading.eq_ignore_ascii_case("project") {
                "project"
            } else {
                "user"
            };
            continue;
        }
        let indent = trimmed.len() - trimmed.trim_start().len();
        let value = trimmed.trim();
        if value.is_empty() {
            continue;
        }
        match indent {
            // Two spaces: a package source. Four spaces: its installed path.
            2 => packages.push(PackageInfo {
                source: value.to_owned(),
                scope: scope.to_owned(),
                path: None,
            }),
            4 => {
                if let Some(last) = packages.last_mut() {
                    last.path = Some(value.to_owned());
                }
            }
            _ => {}
        }
    }
    packages
}

fn cli_error(stdout: &str, stderr: &str) -> anyhow::Error {
    let message = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    anyhow::anyhow!("{message}")
}

/// A source is a single argv entry, so it must never look like a flag. Pi owns the source
/// grammar (npm specs, `git:`, URLs, local paths), so anything else is passed through.
fn plain_source(source: &str) -> Option<String> {
    let source = source.trim();
    let usable = !source.is_empty()
        && source.len() <= 512
        && !source.starts_with('-')
        && !source.chars().any(char::is_control);
    usable.then(|| source.to_owned())
}

async fn run(config: &AgentConfig, args: &[&str]) -> Result<()> {
    let (success, stdout, stderr) = run_agent_command(config, args, Some(PLUGINS_TIMEOUT)).await?;
    if !success {
        return Err(cli_error(&stdout, &stderr));
    }
    Ok(())
}

/// `install`, `remove` and `update` re-list afterwards so the caller sees the new state.
pub async fn install(config: &AgentConfig, source: &str, local: bool) -> Result<Vec<PackageInfo>> {
    let source = plain_source(source).ok_or_else(|| anyhow::anyhow!("包来源不合法"))?;
    let mut args = vec!["install", source.as_str()];
    if local {
        args.push("-l");
    }
    run(config, &args).await?;
    list(config).await
}

pub async fn remove(config: &AgentConfig, source: &str, local: bool) -> Result<Vec<PackageInfo>> {
    let source = plain_source(source).ok_or_else(|| anyhow::anyhow!("包来源不合法"))?;
    let mut args = vec!["remove", source.as_str()];
    if local {
        args.push("-l");
    }
    run(config, &args).await?;
    list(config).await
}

/// Without a source Pi updates every installed package; with one, just that package.
pub async fn update(config: &AgentConfig, source: Option<&str>) -> Result<Vec<PackageInfo>> {
    match source {
        Some(source) => {
            let source = plain_source(source).ok_or_else(|| anyhow::anyhow!("包来源不合法"))?;
            run(config, &["update", "--extension", source.as_str()]).await?;
        }
        None => run(config, &["update", "--extensions"]).await?,
    }
    list(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_scopes_and_an_optional_path() {
        let text = "\
User packages:
  git:github.com/user/repo
    /home/user/.pi/agent/git/github.com/user/repo
  npm:@scope/pkg

Project packages:
  ./local/path
";
        let packages = parse(text);
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[0].scope, "user");
        assert_eq!(
            packages[0].path.as_deref(),
            Some("/home/user/.pi/agent/git/github.com/user/repo")
        );
        // A configured but uninstalled package carries no path.
        assert_eq!(packages[1].source, "npm:@scope/pkg");
        assert!(packages[1].path.is_none());
        assert_eq!(packages[2].scope, "project");
        // An empty store is an empty list, not an error.
        assert!(parse("No packages installed.\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn rejects_a_source_that_could_be_read_as_a_flag() {
        assert!(plain_source("-l").is_none());
        assert!(plain_source("--self").is_none());
        assert!(plain_source("  ").is_none());
        assert!(plain_source("npm:@a/b\u{0}").is_none());
        // Pi owns the source grammar: npm scopes and git/ssh URLs must pass through.
        assert_eq!(plain_source(" npm:@a/b ").as_deref(), Some("npm:@a/b"));
        assert_eq!(
            plain_source("npm:@scope/pkg").as_deref(),
            Some("npm:@scope/pkg")
        );
        assert_eq!(
            plain_source("git:github.com/user/repo").as_deref(),
            Some("git:github.com/user/repo")
        );
        assert_eq!(
            plain_source("ssh://git@github.com/u/r").as_deref(),
            Some("ssh://git@github.com/u/r")
        );
        assert_eq!(
            plain_source("./local/path").as_deref(),
            Some("./local/path")
        );
    }
}
