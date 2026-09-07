use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tokio::{fs, process::Command};

pub const NODE_ROOT: &str = "/opt/agent-runtimes/node";

pub fn supported_node_versions() -> Vec<&'static str> {
    vec!["22.19.0", "22.18.0", "22.17.0", "20.19.4", "20.19.3"]
}

pub fn validate_node_version(version: &str) -> bool {
    supported_node_versions().contains(&version)
}

fn platform_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => Err(anyhow!("unsupported architecture: {other}")),
    }
}

pub fn node_home(version: &str) -> PathBuf {
    Path::new(NODE_ROOT).join(format!("v{version}"))
}
pub fn node_bin(version: &str) -> PathBuf {
    node_home(version).join("bin")
}

pub async fn installed_versions() -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut rd = match fs::read_dir(NODE_ROOT).await {
        Ok(rd) => rd,
        Err(_) => return Ok(out),
    };
    while let Some(entry) = rd.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            let name = entry
                .file_name()
                .to_string_lossy()
                .trim_start_matches('v')
                .to_owned();
            if validate_node_version(&name) && entry.path().join("bin/node").exists() {
                out.push(name);
            }
        }
    }
    out.sort();
    out.reverse();
    Ok(out)
}

pub async fn install_node(version: &str, events: impl Fn(String) + Send + 'static) -> Result<()> {
    if !validate_node_version(version) {
        return Err(anyhow!("unsupported Node.js version: {version}"));
    }
    let arch = platform_arch()?;
    let home = node_home(version);
    if home.join("bin/node").exists() {
        return Ok(());
    }
    fs::create_dir_all(NODE_ROOT).await?;
    let archive = Path::new(NODE_ROOT).join(format!("node-v{version}-linux-{arch}.tar.xz"));
    let url = format!("https://nodejs.org/dist/v{version}/node-v{version}-linux-{arch}.tar.xz");
    events(format!("Downloading Node.js {version} ({arch})..."));
    let status = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(&archive)
        .arg(&url)
        .status()
        .await?;
    if !status.success() {
        return Err(anyhow!("failed to download Node.js {version}"));
    }
    let extract_dir = Path::new(NODE_ROOT).join(format!("extract-{version}"));
    let _ = fs::remove_dir_all(&extract_dir).await;
    fs::create_dir_all(&extract_dir).await?;
    events(format!("Extracting Node.js {version}..."));
    let status = Command::new("tar")
        .args(["-xJf"])
        .arg(&archive)
        .arg("--strip-components=1")
        .arg("-C")
        .arg(&extract_dir)
        .status()
        .await?;
    if !status.success() {
        return Err(anyhow!("failed to extract Node.js {version}"));
    }
    let _ = fs::remove_dir_all(&home).await;
    fs::rename(&extract_dir, &home).await?;
    let _ = fs::remove_file(&archive).await;
    events(format!("Node.js {version} installed."));
    Ok(())
}

pub async fn node_command(version: &str, command: &str) -> Result<Command> {
    if !validate_node_version(version) || !node_bin(version).join("node").exists() {
        return Err(anyhow!("Node.js {version} is not installed"));
    }
    let mut cmd = Command::new(node_bin(version).join(command));
    let path = format!(
        "{}:{}",
        node_bin(version).display(),
        std::env::var("PATH").unwrap_or_default()
    );
    cmd.env("PATH", path)
        .env("NPM_CONFIG_PREFIX", node_home(version));
    Ok(cmd)
}

pub async fn detect_installed_node() -> Result<Option<(String, String)>> {
    for version in installed_versions().await? {
        let mut cmd = node_command(&version, "node").await?;
        let out = cmd.arg("--version").output().await?;
        if out.status.success() {
            return Ok(Some((
                version,
                String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            )));
        }
    }
    Ok(None)
}
