use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Loads one or more `.env` files and merges them into a single HashMap.
///
/// Format per line: `KEY=VALUE` (no `export` prefix). Lines starting with
/// `#` and blank lines are ignored. Values may be optionally quoted with
/// single or double quotes (stripped on parse).
///
/// Files are processed in order; later files override earlier ones.
/// Returns the merged map.
pub fn load_env_files(files: &[String]) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for file_path in files {
        let path = Path::new(file_path);
        let vars = parse_env_file(path)
            .with_context(|| format!("failed to parse env file '{}'", path.display()))?;
        env.extend(vars);
    }
    Ok(env)
}

/// Parses a single `.env` file into a HashMap.
pub fn parse_env_file(path: &Path) -> Result<HashMap<String, String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read env file '{}'", path.display()))?;
    let mut vars = HashMap::new();

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .with_context(|| {
                format!(
                    "invalid env file line {} in '{}': expected KEY=VALUE",
                    line_num + 1,
                    path.display()
                )
            })?;
        let key = key.trim().to_string();
        let value = strip_quotes(value.trim());
        vars.insert(key, value);
    }

    Ok(vars)
}

/// Strips surrounding single or double quotes from a value.
fn strip_quotes(value: &str) -> String {
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

/// Resolves a secret mount source path to an absolute path.
/// Relative paths are resolved against the given `base_dir` (typically
/// the directory containing the manifest).
pub fn resolve_secret_source(source: &Path, base_dir: &Path) -> Result<PathBuf> {
    if source.is_absolute() {
        return Ok(source.to_path_buf());
    }
    let resolved = base_dir.join(source);
    anyhow::ensure!(
        resolved.exists(),
        "secret source file not found: {}",
        resolved.display()
    );
    Ok(resolved)
}

/// Reads a secret file and returns its contents as bytes.
pub fn read_secret(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("failed to read secret file '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_env_file_basic() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "FOO=bar\nBAZ=qux\n# comment\n\nHELLO=world").unwrap();

        let vars = parse_env_file(f.path()).unwrap();
        assert_eq!(vars.get("FOO").unwrap(), "bar");
        assert_eq!(vars.get("BAZ").unwrap(), "qux");
        assert_eq!(vars.get("HELLO").unwrap(), "world");
        assert!(!vars.contains_key("# comment"));
    }

    #[test]
    fn parse_env_file_quoted_values() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "A=\"quoted\"\nB='single'\nC=unquoted").unwrap();

        let vars = parse_env_file(f.path()).unwrap();
        assert_eq!(vars.get("A").unwrap(), "quoted");
        assert_eq!(vars.get("B").unwrap(), "single");
        assert_eq!(vars.get("C").unwrap(), "unquoted");
    }

    #[test]
    fn parse_env_file_value_with_equals() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "CONN=host=localhost;port=5432").unwrap();

        let vars = parse_env_file(f.path()).unwrap();
        assert_eq!(vars.get("CONN").unwrap(), "host=localhost;port=5432");
    }

    #[test]
    fn load_env_files_merges_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("base.env");
        let f2 = dir.path().join("override.env");

        std::fs::write(&f1, "A=1\nB=2\n").unwrap();
        std::fs::write(&f2, "B=99\nC=3\n").unwrap();

        let vars = load_env_files(&[
            f1.to_string_lossy().to_string(),
            f2.to_string_lossy().to_string(),
        ])
        .unwrap();

        assert_eq!(vars.get("A").unwrap(), "1");
        assert_eq!(vars.get("B").unwrap(), "99"); // overridden
        assert_eq!(vars.get("C").unwrap(), "3");
    }

    #[test]
    fn strip_quotes_works() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
        assert_eq!(strip_quotes("'hello'"), "hello");
        assert_eq!(strip_quotes("hello"), "hello");
        assert_eq!(strip_quotes("\"partial"), "\"partial"); // no closing quote
    }
}
