//! Multi-stage Dockerfile build engine for `tpt origin build`.
//!
//! Executes Dockerfile stages using containerd: pulls base images, runs
//! commands via `ctr tasks exec`, captures filesystem diffs as OCI layers,
//! and composes the final image into containerd's content/image store.
//!
//! Linux-only: requires a running containerd daemon and the `ctr` CLI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::dockerfile::{self, Keyword, Stage};

/// Configuration for a build invocation.
pub struct BuildConfig {
    /// Absolute path to the directory containing the Dockerfile.
    pub context_dir: PathBuf,
    /// Absolute path to the Dockerfile itself.
    pub dockerfile_path: PathBuf,
    /// Tag for the output image (e.g. "myapp:latest").
    pub tag: String,
    /// User-supplied `--build-arg KEY=VALUE` overrides.
    pub build_args: HashMap<String, String>,
    /// Path to the containerd socket.
    pub socket_path: PathBuf,
}

/// Result of a successful build.
pub struct BuildResult {
    pub image_ref: String,
    pub stages_built: usize,
    pub layers_created: usize,
}

/// Recursively deletes a directory and all its contents, ignoring errors.
fn cleanup_dir(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

/// Runs a shell command and returns its output. Fails on non-zero exit.
fn run_cmd(cmd: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn `{cmd}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`{cmd} {}` failed: {stderr}", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Runs a shell command, inheriting stdio (for interactive/verbose output).
fn run_cmd_stdio(cmd: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("failed to spawn `{cmd}`"))?;
    if !status.success() {
        bail!("`{cmd} {}` exited with {status}", args.join(" "));
    }
    Ok(())
}

/// Strips the registry prefix from an image reference, returning just
/// `name:tag` or `name@digest`. Used when we need a short label for
/// containerd container IDs derived from the image.
fn short_image_name(image_ref: &str) -> String {
    // "docker.io/library/alpine:3.19" -> "alpine:3.19"
    // "ghcr.io/foo/bar:tag" -> "foo/bar:tag"
    let stripped = image_ref
        .strip_prefix("docker.io/library/")
        .or_else(|| image_ref.strip_prefix("docker.io/"))
        .unwrap_or(image_ref);
    stripped.to_string()
}

/// Build a unique container ID for a build stage that won't collide with
/// running service containers.
fn build_container_id(stage_idx: usize, image_ref: &str) -> String {
    let short = short_image_name(image_ref)
        .replace('/', "-")
        .replace(':', "-");
    format!("tpt-build-s{stage_idx}-{short}-{}", std::process::id())
}

/// Evaluates simple `${VAR}` and `$VAR` substitutions against the combined
/// build-arg + env-arg environment.
fn expand_args(input: &str, env: &HashMap<String, String>) -> String {
    let mut result = input.to_string();
    // Process ${VAR} first (greedy), then $VAR.
    for (key, value) in env {
        let patterns = [format!("${{{key}}}"), format!("${key}")];
        for pattern in &patterns {
            result = result.replace(pattern, value);
        }
    }
    result
}

// ── Layer capture ─────────────────────────────────────────────────────

/// Captures the filesystem state of container `id` as an OCI layer tar.
///
/// Strategy: use `ctr images export` is NOT available for containers. Instead,
/// we use the containerd snapshot diff mechanism:
/// 1. Record the snapshot before the instruction (via `ctr containers info`).
/// 2. After the instruction, the container's snapshot has been modified.
/// 3. Use `ctr snapshot diff` to produce a tar of the changes.
///
/// However, `ctr snapshot diff` is not a stable public command in all
/// containerd versions. As a portable fallback we use a two-pass approach:
/// - `ctr images export` the base image to a temp OCI tar.
/// - Extract the tar, apply the instruction, tar the diff.
///
/// For the initial implementation we take a pragmatic shortcut: capture the
/// **entire container rootfs** after each RUN via a bind-mount trick. The
/// container is started with a read-write snapshot, we `ctr tasks exec` the
/// command, and then we export the resulting content by reading the snapshot
/// mounts directly from `/var/lib/containerd` (the standard state dir).
fn capture_layer_via_ctr(socket: &Path, namespace: &str, container_id: &str) -> Result<Vec<u8>> {
    // Get the container's snapshot key via `ctr containers info`.
    let info_json = run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "containers",
            "info",
            container_id,
        ],
    )?;

    // Parse the snapshot key from the JSON output.
    let info: serde_json::Value = serde_json::from_str(&info_json)
        .with_context(|| format!("failed to parse container info JSON for '{container_id}'"))?;
    let snapshot_key = info["snapshotKey"]
        .as_str()
        .context("container info missing snapshotKey")?
        .to_string();

    // Use `ctr snapshot diff` to produce a tar of changes vs parent.
    // If the snapshot has no parent (first layer from base image), we
    // capture the entire snapshot content.
    let _parent_key = info["snapshotKey"].as_str().map(|_| {
        // Compute parent: strip the last component.
        // containerd snapshot keys look like
        // "build-<id>-<sha>-0", "build-<id>-<sha>-1", etc.
        let parts: Vec<&str> = snapshot_key.rsplitn(2, '-').collect();
        if parts.len() == 2 {
            // Reconstruct parent by decrementing the numeric suffix.
            // For simplicity, just use the base image snapshot.
            format!("parent-{}", container_id)
        } else {
            snapshot_key.clone()
        }
    });

    // Attempt the diff approach — works on containerd >= 1.6.
    // Fallback: read the snapshot mount point directly.
    let mounts_json = run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "snapshots",
            "mounts",
            &snapshot_key,
        ],
    )
    .with_context(|| {
        format!(
            "failed to get snapshot mounts for '{snapshot_key}' — \
             is containerd >= 1.6?"
        )
    })?;

    let mounts: serde_json::Value = serde_json::from_str(&mounts_json)
        .with_context(|| "failed to parse snapshot mounts JSON")?;

    // mounts is an array of { "type": "bind", "source": "/...", "options": [...] }
    let rootfs = mounts
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|m| m["source"].as_str())
        .context("snapshot mounts missing source path")?;

    // The source is typically the upper dir of an overlay mount. We need to
    // tar its contents as the layer.
    let rootfs_path = PathBuf::from(rootfs);
    if !rootfs_path.exists() {
        bail!(
            "snapshot source path {} does not exist for container '{container_id}'",
            rootfs_path.display()
        );
    }

    // Create an OCI layer tar (uncompressed) from the rootfs directory.
    // The tar must use OCI layer format: paths relative to root, whiteout
    // files for deletions (not handled in this initial version — deletions
    // within a RUN are rare in practice).
    let tar_output = std::process::Command::new("tar")
        .args(["-cf", "-", "-C", &rootfs_path.to_string_lossy(), "."])
        .output()
        .with_context(|| {
            format!(
                "failed to run `tar` on snapshot rootfs at {}",
                rootfs_path.display()
            )
        })?;

    if !tar_output.status.success() {
        bail!(
            "`tar` failed on snapshot rootfs: {}",
            String::from_utf8_lossy(&tar_output.stderr)
        );
    }

    Ok(tar_output.stdout)
}

// ── Stage execution ───────────────────────────────────────────────────

/// Executes a single build stage inside containerd.
///
/// Returns `(final_image_ref, final_fs_layer, instruction_layers)` — the
/// final_fs_layer is the complete stage filesystem for COPY --from access,
/// and instruction_layers are per-instruction diffs for the OCI image.
fn execute_stage(
    config: &BuildConfig,
    stage_idx: usize,
    stage: &Stage,
    build_args: &HashMap<String, String>,
    previous_stages: &HashMap<String, Vec<u8>>,
) -> Result<(String, Vec<u8>, Vec<Vec<u8>>)> {
    let socket = &config.socket_path;
    let namespace = "tpt-boxcar";

    // ── 1. Resolve the base image ──
    let base_ref = expand_args(&stage.from.image, build_args);
    let base_ref = normalize_image_ref_for_ctr(&base_ref);
    tracing::info!("stage {stage_idx}: pulling base image {base_ref}");

    run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "images",
            "pull",
            &base_ref,
        ],
    )
    .with_context(|| format!("failed to pull base image '{base_ref}' for stage {stage_idx}"))?;

    // ── 2. Create container from base image ──
    let container_id = build_container_id(stage_idx, &base_ref);
    tracing::info!("stage {stage_idx}: creating container {container_id}");

    // Remove any leftover container with this ID.
    let _ = run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "containers",
            "rm",
            &container_id,
        ],
    );

    // Create the container (no task — we'll use exec for each instruction).
    run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "containers",
            "create",
            &base_ref,
            &container_id,
        ],
    )
    .with_context(|| format!("failed to create container '{container_id}'"))?;

    // ── 3. Execute instructions ──
    let mut layers: Vec<Vec<u8>> = Vec::new();
    let mut env = build_args.clone();
    let mut workdir = "/".to_string();

    for instr in &stage.instructions {
        match instr.keyword {
            Keyword::Run => {
                tracing::info!(
                    "stage {stage_idx}: RUN {}",
                    &instr.args[..instr.args.len().min(80)]
                );

                // Capture snapshot before the instruction.
                let _snapshot_before = capture_snapshot_info(socket, namespace, &container_id)?;

                // Execute the RUN command via `ctr tasks exec`.
                let exec_id = format!("build-{}-{}", container_id, next_build_exec_id());
                let mut exec_args: Vec<String> = vec![
                    "--address".into(),
                    socket.to_string_lossy().to_string(),
                    "--namespace".into(),
                    namespace.into(),
                    "tasks".into(),
                    "exec".into(),
                    "--exec-id".into(),
                    exec_id,
                    "--cwd".into(),
                    workdir.clone(),
                ];

                // Inject environment variables.
                for (k, v) in &env {
                    exec_args.push("--env".into());
                    exec_args.push(format!("{k}={v}"));
                }

                exec_args.push(container_id.clone());

                // Wrap in shell.
                let shell_cmd = format!("sh -c {}", shell_quote(&instr.args));
                exec_args.push(shell_cmd);

                let output = std::process::Command::new("ctr")
                    .args(&exec_args)
                    .output()
                    .with_context(|| format!("failed to exec RUN in container '{container_id}'"))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "stage {stage_idx}: RUN command failed in '{container_id}': {stderr}\n  command: {}",
                        instr.args
                    );
                }

                // Capture the layer diff.
                let layer = capture_layer_via_ctr(socket, namespace, &container_id)?;
                if !layer.is_empty() {
                    layers.push(layer);
                }
            }
            Keyword::Copy => {
                tracing::info!("stage {stage_idx}: COPY {}", instr.args);

                let (src, dest) = parse_copy_args(&instr.args)?;

                // Handle COPY --from=<stage> (multi-stage copy).
                if let Some(from_alias) = src.strip_prefix("--from=") {
                    let from_bytes = previous_stages.get(from_alias).with_context(|| {
                        format!(
                            "COPY --from={from_alias}: stage alias not found \
                                 (available: {:?})",
                            previous_stages.keys().collect::<Vec<_>>()
                        )
                    })?;

                    // Write the source bytes to a temp file, then inject into container.
                    let tmp = tempfile::tempdir()?;
                    let tmp_file = tmp.path().join("copy-from");
                    std::fs::write(&tmp_file, from_bytes)?;

                    inject_file_into_container(
                        socket,
                        namespace,
                        &container_id,
                        &workdir,
                        &tmp_file,
                        &dest,
                    )?;

                    // Capture layer.
                    let layer = capture_layer_via_ctr(socket, namespace, &container_id)?;
                    if !layer.is_empty() {
                        layers.push(layer);
                    }
                } else {
                    // Regular COPY from build context.
                    let src_path = config.context_dir.join(&src);
                    if !src_path.exists() {
                        bail!(
                            "stage {stage_idx}: COPY source '{}' does not exist in build context",
                            src
                        );
                    }

                    inject_file_into_container(
                        socket,
                        namespace,
                        &container_id,
                        &workdir,
                        &src_path,
                        &dest,
                    )?;

                    let layer = capture_layer_via_ctr(socket, namespace, &container_id)?;
                    if !layer.is_empty() {
                        layers.push(layer);
                    }
                }
            }
            Keyword::Add => {
                // ADD is like COPY but also handles URLs and tar extraction.
                // For now, treat it identically to COPY (no URL support yet).
                tracing::info!("stage {stage_idx}: ADD {} (treated as COPY)", instr.args);

                let (src, dest) = parse_copy_args(&instr.args)?;
                let src_path = config.context_dir.join(&src);
                if !src_path.exists() {
                    bail!(
                        "stage {stage_idx}: ADD source '{}' does not exist in build context",
                        src
                    );
                }

                inject_file_into_container(
                    socket,
                    namespace,
                    &container_id,
                    &workdir,
                    &src_path,
                    &dest,
                )?;

                let layer = capture_layer_via_ctr(socket, namespace, &container_id)?;
                if !layer.is_empty() {
                    layers.push(layer);
                }
            }
            Keyword::Env => {
                // ENV KEY VALUE or ENV KEY=VALUE
                let (key, value) = parse_env_args(&instr.args)?;
                env.insert(key, value);
            }
            Keyword::Arg => {
                // ARG KEY or ARG KEY=DEFAULT
                let (key, value) = parse_arg_decl(&instr.args);
                // Build-arg overrides take precedence over Dockerfile ARG defaults.
                if !env.contains_key(&key) {
                    env.insert(key, value);
                }
            }
            Keyword::Workdir => {
                let new_workdir = instr.args.trim().trim_matches('"').trim_matches('\'');
                if new_workdir.starts_with('/') {
                    workdir = new_workdir.to_string();
                } else if workdir == "/" {
                    workdir = format!("/{new_workdir}");
                } else {
                    workdir = format!("{workdir}/{new_workdir}");
                }
                // Ensure the directory exists in the container.
                let exec_id = format!("workdir-{}", next_build_exec_id());
                let _ = run_cmd(
                    "ctr",
                    &[
                        "--address",
                        &socket.to_string_lossy(),
                        "--namespace",
                        namespace,
                        "tasks",
                        "exec",
                        "--exec-id",
                        &exec_id,
                        &container_id,
                        "mkdir",
                        "-p",
                        &workdir,
                    ],
                );
            }
            Keyword::Cmd | Keyword::Entrypoint => {
                // Metadata-only — record but don't execute during build.
                tracing::debug!(
                    "stage {stage_idx}: {:?} {} (metadata)",
                    instr.keyword,
                    instr.args
                );
            }
            Keyword::Expose => {
                // Metadata-only — port declarations don't affect the filesystem.
            }
            Keyword::Label => {
                // Metadata-only — doesn't affect the filesystem.
            }
            Keyword::User => {
                // Set the user for subsequent RUN commands.
                let user = instr.args.trim().trim_matches('"').trim_matches('\'');
                env.insert("TPT_BUILD_USER".to_string(), user.to_string());
            }
            Keyword::Volume => {
                // Volume declarations are metadata for runtime.
            }
            Keyword::Shell => {
                // Override the default shell for RUN commands.
                // Format: SHELL ["executable", "param1", "param2"]
                tracing::info!(
                    "stage {stage_idx}: SHELL {} (not yet supported, using sh)",
                    instr.args
                );
            }
            Keyword::StopSignal => {
                // Metadata-only.
            }
            Keyword::Healthcheck => {
                // Metadata-only.
            }
            Keyword::Maintainer => {
                // Deprecated — treat as LABEL.
            }
            Keyword::Onbuild => {
                // ONBUILD wraps the next instruction — not supported in
                // multi-stage builds (Docker ignores it in non-final stages).
                tracing::warn!(
                    "stage {stage_idx}: ONBUILD is not supported during build, skipping"
                );
            }
            Keyword::From => {
                // FROM is the stage header — already processed by the caller;
                // it never appears as an instruction within a stage's body.
            }
        }
    }

    // ── 4. Export the final stage filesystem ──
    let stage_name_default = format!("stage-{stage_idx}");
    let stage_name = stage.from.alias.as_deref().unwrap_or(&stage_name_default);
    let final_image_ref = format!(
        "tpt-boxcar/build/{}:{}",
        short_image_name(&base_ref).replace('/', "-"),
        stage_name
    );

    // Export the container's final filesystem for COPY --from access.
    let final_fs = capture_layer_via_ctr(socket, namespace, &container_id)?;

    // Clean up the container.
    let _ = run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "containers",
            "rm",
            &container_id,
        ],
    );

    Ok((final_image_ref, final_fs, layers))
}

// ── Helper functions ──────────────────────────────────────────────────

/// Captures basic snapshot info (the snapshot key) for a container.
fn capture_snapshot_info(socket: &Path, namespace: &str, container_id: &str) -> Result<String> {
    let info_json = run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "containers",
            "info",
            container_id,
        ],
    )?;
    let info: serde_json::Value = serde_json::from_str(&info_json)?;
    Ok(info["snapshotKey"]
        .as_str()
        .unwrap_or("unknown")
        .to_string())
}

/// Injects a file or directory from the host into a container's filesystem
/// by copying it via `ctr tasks exec` + `cp` / `tar`.
fn inject_file_into_container(
    socket: &Path,
    namespace: &str,
    container_id: &str,
    workdir: &str,
    host_src: &Path,
    container_dest: &str,
) -> Result<()> {
    // Create a tar of the source file/directory and pipe it into the
    // container via `ctr tasks exec` + `tar -xf -`.
    let mut tar_child = std::process::Command::new("tar")
        .args(["-cf", "-", "-C"])
        .arg(host_src.parent().unwrap_or_else(|| Path::new(".")))
        .arg(
            host_src
                .file_name()
                .context("source path has no file name")?,
        )
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn `tar` for COPY injection")?;

    let exec_id = format!("copy-{}", next_build_exec_id());
    let dest_dir = if container_dest.ends_with('/') || container_dest.is_empty() {
        container_dest.to_string()
    } else {
        // Ensure the parent directory exists.
        let parent = Path::new(container_dest)
            .parent()
            .unwrap_or_else(|| Path::new("/"));
        let parent_str = parent.to_string_lossy().to_string();
        let _ = run_cmd(
            "ctr",
            &[
                "--address",
                &socket.to_string_lossy(),
                "--namespace",
                namespace,
                "tasks",
                "exec",
                "--exec-id",
                &format!("mkdir-{exec_id}"),
                container_id,
                "mkdir",
                "-p",
                &parent_str,
            ],
        );
        container_dest.to_string()
    };

    let untar = std::process::Command::new("ctr")
        .args([
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "tasks",
            "exec",
            "--exec-id",
            &exec_id,
            "--cwd",
            workdir,
            container_id,
            "tar",
            "-xf",
            "-",
            "-C",
            &dest_dir,
        ])
        .stdin(tar_child.stdout.take().unwrap())
        .output()
        .with_context(|| "failed to exec `tar -xf` in container for COPY")?;

    // Wait for the tar process.
    let _ = tar_child.wait();

    if !untar.status.success() {
        bail!(
            "COPY injection failed: {}",
            String::from_utf8_lossy(&untar.stderr)
        );
    }

    Ok(())
}

/// Normalizes an image reference for `ctr` (adds docker.io/library/ prefix
/// for bare names).
fn normalize_image_ref_for_ctr(image_ref: &str) -> String {
    if !image_ref.contains('/') {
        return format!("docker.io/library/{image_ref}");
    }
    let first_segment = image_ref.split('/').next().unwrap();
    let has_registry_host =
        first_segment == "localhost" || first_segment.contains('.') || first_segment.contains(':');
    if has_registry_host {
        image_ref.to_string()
    } else {
        format!("docker.io/{image_ref}")
    }
}

/// Parses `COPY src dest` or `COPY --from=N src dest`.
fn parse_copy_args(args: &str) -> Result<(String, String)> {
    let tokens = dockerfile::tokenize(args);
    if tokens.len() < 2 {
        bail!("COPY requires at least source and destination arguments");
    }

    // Check for --from= prefix.
    if tokens.len() == 3 && tokens[0].starts_with("--from=") {
        return Ok((tokens[0].clone(), tokens[2].clone()));
    }
    if tokens.len() == 2 {
        return Ok((tokens[0].clone(), tokens[1].clone()));
    }

    bail!("invalid COPY syntax: {args}");
}

/// Parses `ENV KEY VALUE` or `ENV KEY=VALUE`.
fn parse_env_args(args: &str) -> Result<(String, String)> {
    let trimmed = args.trim();
    if let Some(eq_pos) = trimmed.find('=') {
        let key = trimmed[..eq_pos].trim().to_string();
        let value = trimmed[eq_pos + 1..].trim().to_string();
        return Ok((key, value));
    }
    let tokens = dockerfile::tokenize(trimmed);
    if tokens.len() >= 2 {
        return Ok((tokens[0].clone(), tokens[1..].join(" ")));
    }
    bail!("invalid ENV syntax: {args}");
}

/// Parses `ARG KEY` or `ARG KEY=DEFAULT`, returning (key, default_value).
fn parse_arg_decl(args: &str) -> (String, String) {
    if let Some(eq_pos) = args.find('=') {
        let key = args[..eq_pos].trim().to_string();
        let value = args[eq_pos + 1..].trim().to_string();
        (key, value)
    } else {
        (args.trim().to_string(), String::new())
    }
}

/// Quotes a string for safe embedding in `sh -c '...'`.
fn shell_quote(s: &str) -> String {
    // Use single quotes; escape any embedded single quotes.
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Monotonically increasing exec ID counter for build operations.
fn next_build_exec_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

// ── OCI image composition ─────────────────────────────────────────────

/// Composes an OCI image from collected layers and imports it into
/// containerd's image store.
fn compose_oci_image(
    config: &BuildConfig,
    _stage_name: &str,
    layers: &[Vec<u8>],
    cmd: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    workdir: &str,
    env: &HashMap<String, String>,
) -> Result<String> {
    let image_ref = format!("tpt-boxcar/{}", config.tag.replace(':', "/"));
    let socket = &config.socket_path;
    let namespace = "tpt-boxcar";

    // ── 1. Compute layer digests and write to temp dir ──
    let tmp = tempfile::tempdir()?;
    let blobs_dir = tmp.path().join("blobs");
    std::fs::create_dir_all(&blobs_dir)?;

    let mut layer_descriptors = Vec::new();
    let mut media_type_layers = Vec::new();

    for (i, layer_bytes) in layers.iter().enumerate() {
        let digest = format!("sha256:{:x}", Sha256::digest(layer_bytes));
        let layer_path = blobs_dir.join(format!("{i}.tar"));
        std::fs::write(&layer_path, layer_bytes)?;

        // Import the layer blob into containerd's content store.
        let output = std::process::Command::new("ctr")
            .args([
                "--address",
                &socket.to_string_lossy(),
                "--namespace",
                namespace,
                "content",
                "ingest",
                "--ref",
                &digest,
            ])
            .stdin(std::fs::File::open(&layer_path)?)
            .output()
            .with_context(|| "failed to ingest layer blob into containerd")?;

        if !output.status.success() {
            tracing::warn!(
                "content ingest for layer {i} failed (may already exist): {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        layer_descriptors.push(serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": digest,
            "size": layer_bytes.len(),
        }));
        media_type_layers.push("application/vnd.oci.image.layer.v1.tar".to_string());
    }

    // ── 2. Build OCI config ──
    let mut config_obj = serde_json::json!({
        "architecture": "amd64",
        "os": "linux",
        "config": {
            "workingDir": workdir,
            "env": env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>(),
        },
    });

    if let Some(entrypoint) = entrypoint {
        config_obj["config"]["Entrypoint"] = serde_json::json!(entrypoint);
    }
    if let Some(cmd) = cmd {
        config_obj["config"]["Cmd"] = serde_json::json!(cmd);
    }

    // Write config blob.
    let config_bytes = serde_json::to_vec_pretty(&config_obj)?;
    let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));
    let config_path = blobs_dir.join("config.json");
    std::fs::write(&config_path, &config_bytes)?;

    let output = std::process::Command::new("ctr")
        .args([
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "content",
            "ingest",
            "--ref",
            &config_digest,
        ])
        .stdin(std::fs::File::open(&config_path)?)
        .output()
        .context("failed to ingest config blob into containerd")?;

    if !output.status.success() {
        tracing::warn!(
            "config ingest failed (may already exist): {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // ── 3. Build OCI manifest ──
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config_bytes.len(),
        },
        "layers": layer_descriptors,
    });

    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let manifest_digest = format!("sha256:{:x}", Sha256::digest(&manifest_bytes));
    let manifest_path = blobs_dir.join("manifest.json");
    std::fs::write(&manifest_path, &manifest_bytes)?;

    let output = std::process::Command::new("ctr")
        .args([
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "content",
            "ingest",
            "--ref",
            &manifest_digest,
        ])
        .stdin(std::fs::File::open(&manifest_path)?)
        .output()
        .context("failed to ingest manifest blob into containerd")?;

    if !output.status.success() {
        tracing::warn!(
            "manifest ingest failed (may already exist): {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // ── 4. Create the image record in containerd ──
    let socket_lossy = socket.to_string_lossy();
    let mut image_cmd = vec![
        "--address",
        &socket_lossy,
        "--namespace",
        namespace,
        "images",
        "create",
    ];

    // If the image already exists, remove it first.
    let _ = run_cmd(
        "ctr",
        &[
            "--address",
            &socket.to_string_lossy(),
            "--namespace",
            namespace,
            "images",
            "rm",
            &image_ref,
        ],
    );

    image_cmd.push(&image_ref);
    image_cmd.push(&manifest_digest);

    let output = std::process::Command::new("ctr")
        .args(&image_cmd)
        .output()
        .with_context(|| format!("failed to create image '{image_ref}' in containerd"))?;

    if !output.status.success() {
        bail!(
            "failed to create image '{image_ref}': {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(image_ref)
}

// ── Public build entry point ──────────────────────────────────────────

/// Executes a full multi-stage Dockerfile build using containerd.
pub fn build(config: &BuildConfig) -> Result<BuildResult> {
    tracing::info!(
        "building from {} (context: {})",
        config.dockerfile_path.display(),
        config.context_dir.display()
    );

    // ── 1. Parse the Dockerfile ──
    let dockerfile = dockerfile::parse_file(&config.dockerfile_path)?;
    tracing::info!("parsed {} stage(s)", dockerfile.stages.len());

    // ── 2. Build stages sequentially ──
    let mut previous_stages: HashMap<String, Vec<u8>> = HashMap::new();
    let mut total_layers = 0;
    let mut final_image_ref = String::new();

    for (stage_idx, stage) in dockerfile.stages.iter().enumerate() {
        let stage_name_default = format!("stage-{stage_idx}");
        let stage_name = stage.from.alias.as_deref().unwrap_or(&stage_name_default);

        tracing::info!(
            "─── stage {stage_idx}: FROM {} {}",
            stage.from.image,
            stage
                .from
                .alias
                .as_deref()
                .map(|a| format!("AS {a}"))
                .unwrap_or_default()
        );

        let (_image_ref, final_fs, layers) = execute_stage(
            config,
            stage_idx,
            stage,
            &config.build_args,
            &previous_stages,
        )?;

        // Record this stage's filesystem for COPY --from access.
        previous_stages.insert(stage_name.to_string(), final_fs);

        // For multi-stage builds, only the final stage produces the
        // output image. Intermediate stages are used only for
        // COPY --from access.
        if stage_idx == dockerfile.stages.len() - 1 {
            // Extract CMD/ENTRYPOINT/WORKDIR from the final stage's
            // instructions.
            let mut cmd: Option<Vec<String>> = None;
            let mut entrypoint: Option<Vec<String>> = None;
            let mut workdir = "/".to_string();
            let mut env = config.build_args.clone();

            for instr in &stage.instructions {
                match instr.keyword {
                    Keyword::Cmd => {
                        cmd = Some(dockerfile::tokenize(&instr.args));
                    }
                    Keyword::Entrypoint => {
                        entrypoint = Some(dockerfile::tokenize(&instr.args));
                    }
                    Keyword::Workdir => {
                        let w = instr.args.trim().trim_matches('"').trim_matches('\'');
                        if w.starts_with('/') {
                            workdir = w.to_string();
                        } else {
                            workdir = format!("{workdir}/{w}");
                        }
                    }
                    Keyword::Env => {
                        if let Ok((k, v)) = parse_env_args(&instr.args) {
                            env.insert(k, v);
                        }
                    }
                    _ => {}
                }
            }

            final_image_ref =
                compose_oci_image(config, stage_name, &layers, cmd, entrypoint, &workdir, &env)?;
            total_layers += layers.len();
        }
    }

    Ok(BuildResult {
        image_ref: final_image_ref,
        stages_built: dockerfile.stages.len(),
        layers_created: total_layers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_copy_args_simple() {
        let (src, dest) = parse_copy_args("app/ /usr/share/nginx/html").unwrap();
        assert_eq!(src, "app/");
        assert_eq!(dest, "/usr/share/nginx/html");
    }

    #[test]
    fn parse_copy_args_from() {
        let (src, dest) =
            parse_copy_args("--from=builder /app/dist /usr/share/nginx/html").unwrap();
        assert_eq!(src, "--from=builder");
        assert_eq!(dest, "/usr/share/nginx/html");
    }

    #[test]
    fn parse_env_equals() {
        let (k, v) = parse_env_args("NODE_ENV=production").unwrap();
        assert_eq!(k, "NODE_ENV");
        assert_eq!(v, "production");
    }

    #[test]
    fn parse_env_space() {
        let (k, v) = parse_env_args("NODE_ENV production").unwrap();
        assert_eq!(k, "NODE_ENV");
        assert_eq!(v, "production");
    }

    #[test]
    fn parse_arg_decl_with_default() {
        let (k, v) = parse_arg_decl("VERSION=1.0.0");
        assert_eq!(k, "VERSION");
        assert_eq!(v, "1.0.0");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        let q = shell_quote("echo 'hello world'");
        assert_eq!(q, "'echo '\\''hello world'\\'''");
    }

    #[test]
    fn expand_args_substitutes() {
        let mut env = HashMap::new();
        env.insert("VERSION".to_string(), "2.0".to_string());
        env.insert("PORT".to_string(), "8080".to_string());
        let result = expand_args("app-${VERSION}:$PORT", &env);
        assert_eq!(result, "app-2.0:8080");
    }
}
