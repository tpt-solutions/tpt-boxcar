//! Dockerfile parser for `tpt origin build`.
//!
//! Parses Dockerfile syntax into structured instructions, handling line
//! continuations, comments, quoted arguments, and multi-stage `FROM ... AS`
//! aliases. The parser is deliberately dependency-free (no pest/nom/lalrpop)
//! to keep the build graph self-contained.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// A single parsed Dockerfile instruction.
#[derive(Debug, Clone)]
pub struct Instruction {
    pub keyword: Keyword,
    pub args: String,
    pub line: usize,
}

/// Recognized Dockerfile keywords.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keyword {
    From,
    Run,
    Cmd,
    Label,
    Maintainer,
    Expose,
    Env,
    Add,
    Copy,
    Entrypoint,
    Volume,
    User,
    Workdir,
    Arg,
    Onbuild,
    StopSignal,
    Healthcheck,
    Shell,
    // Passthrough — we parse them but don't execute in the build engine.
}

impl Keyword {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "FROM" => Some(Keyword::From),
            "RUN" => Some(Keyword::Run),
            "CMD" => Some(Keyword::Cmd),
            "LABEL" => Some(Keyword::Label),
            "MAINTAINER" => Some(Keyword::Maintainer),
            "EXPOSE" => Some(Keyword::Expose),
            "ENV" => Some(Keyword::Env),
            "ADD" => Some(Keyword::Add),
            "COPY" => Some(Keyword::Copy),
            "ENTRYPOINT" => Some(Keyword::Entrypoint),
            "VOLUME" => Some(Keyword::Volume),
            "USER" => Some(Keyword::User),
            "WORKDIR" => Some(Keyword::Workdir),
            "ARG" => Some(Keyword::Arg),
            "ONBUILD" => Some(Keyword::Onbuild),
            "STOPSIGNAL" => Some(Keyword::StopSignal),
            "HEALTHCHECK" => Some(Keyword::Healthcheck),
            "SHELL" => Some(Keyword::Shell),
            _ => None,
        }
    }
}

/// A parsed `FROM` line, broken into its components.
#[derive(Debug, Clone)]
pub struct FromDirective {
    pub image: String,
    pub alias: Option<String>,
    pub platform: Option<String>,
}

/// A parsed Dockerfile, consisting of one or more stages.
#[derive(Debug, Clone)]
pub struct Dockerfile {
    pub stages: Vec<Stage>,
}

/// A single build stage (delimited by `FROM`).
#[derive(Debug, Clone)]
pub struct Stage {
    pub from: FromDirective,
    pub instructions: Vec<Instruction>,
}

/// Tokenizes a Dockerfile string into raw lines, handling `\` continuations
/// and stripping `#` comments and blank lines.
fn preprocess(input: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut continuation = String::new();
    let mut line_num: usize = 0;

    for raw_line in input.lines() {
        line_num += 1;
        let trimmed = raw_line.trim_end();

        if continuation.is_empty() {
            // Skip blank lines and full-line comments.
            let stripped = trimmed.trim();
            if stripped.is_empty() || stripped.starts_with('#') {
                continue;
            }
        }

        if let Some(without_backslash) = trimmed.strip_suffix('\\') {
            // Continuation: strip trailing `\` and accumulate.
            continuation.push_str(without_backslash);
            continuation.push(' ');
        } else {
            continuation.push_str(trimmed);
            lines.push((
                line_num - continuation.matches('\n').count(),
                continuation.trim().to_string(),
            ));
            continuation.clear();
        }
    }

    // Handle unterminated continuation (treat as final line).
    if !continuation.trim().is_empty() {
        lines.push((line_num, continuation.trim().to_string()));
    }

    lines
}

/// Splits a line into (keyword, rest_of_line), returning `None` for comment
/// or blank lines (which should already be filtered by `preprocess`).
fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let chars = trimmed.char_indices();
    // Find end of keyword (first whitespace).
    for (i, c) in chars {
        if c.is_whitespace() {
            let keyword = &trimmed[..i];
            let rest = trimmed[i..].trim();
            return Some((keyword, rest));
        }
    }
    // Entire line is a keyword with no args.
    Some((trimmed, ""))
}

/// Parses `FROM image[:tag] [AS alias] [--platform=...]` into components.
pub fn parse_from(args: &str) -> Result<FromDirective> {
    let mut image = None;
    let mut alias = None;
    let mut platform = None;

    // Tokenize respecting quoted strings.
    let tokens = tokenize(args);
    for token in &tokens {
        if token.eq_ignore_ascii_case("AS") {
            // Next token is the alias.
            continue;
        }
        if let Some(prev) = tokens.iter().position(|t| t == token) {
            if prev > 0 && tokens[prev - 1].eq_ignore_ascii_case("AS") {
                alias = Some(token.clone());
                continue;
            }
        }
        if let Some(rest) = token.strip_prefix("--platform=") {
            platform = Some(rest.to_string());
            continue;
        }
        // First unrecognized token is the image.
        if image.is_none() {
            image = Some(token.clone());
        }
    }

    Ok(FromDirective {
        image: image
            .context("FROM directive is missing an image reference")?
            .to_string(),
        alias,
        platform,
    })
}

/// Tokenizes a space-separated argument string, respecting single and double
/// quotes. Quoted strings are returned without their surrounding quotes.
pub fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in input.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Parses a complete Dockerfile into stages.
pub fn parse(input: &str) -> Result<Dockerfile> {
    let lines = preprocess(input);
    let mut stages: Vec<Stage> = Vec::new();
    let mut current_instructions: Vec<Instruction> = Vec::new();
    let mut current_from: Option<FromDirective> = None;

    for (line_num, raw) in &lines {
        let (keyword_str, args) = split_keyword(raw)
            .with_context(|| format!("line {line_num}: unable to parse instruction"))?;

        let keyword = Keyword::parse(keyword_str)
            .with_context(|| format!("line {line_num}: unknown instruction '{keyword_str}'"))?;

        if keyword == Keyword::From {
            // Finalize the previous stage if any.
            if let Some(from) = current_from.take() {
                stages.push(Stage {
                    from,
                    instructions: std::mem::take(&mut current_instructions),
                });
            }
            let from = parse_from(args)
                .with_context(|| format!("line {line_num}: invalid FROM directive"))?;
            current_from = Some(from);
        } else if keyword == Keyword::Onbuild {
            // ONBUILD wraps the next instruction — store as-is.
            current_instructions.push(Instruction {
                keyword,
                args: args.to_string(),
                line: *line_num,
            });
        } else {
            current_instructions.push(Instruction {
                keyword,
                args: args.to_string(),
                line: *line_num,
            });
        }
    }

    // Push the final stage.
    if let Some(from) = current_from.take() {
        stages.push(Stage {
            from,
            instructions: current_instructions,
        });
    }

    if stages.is_empty() {
        bail!("Dockerfile contains no FROM instructions");
    }

    Ok(Dockerfile { stages })
}

/// Parses a Dockerfile from a file path.
pub fn parse_file(path: &Path) -> Result<Dockerfile> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read Dockerfile at {}", path.display()))?;
    parse(&content).with_context(|| format!("failed to parse {}", path.display()))
}

/// Resolves a `COPY` or `ADD` source path relative to the build context,
/// returning the absolute host path.
pub fn resolve_build_context_path(context_dir: &Path, src: &str) -> PathBuf {
    // Handle absolute paths (e.g. COPY /etc/hosts ...) — not common but valid.
    if src.starts_with('/') {
        return PathBuf::from(src);
    }
    context_dir.join(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_two_stage_dockerfile() {
        let input = r#"
FROM node:20-alpine AS builder
WORKDIR /app
COPY package.json .
RUN npm install
COPY . .
RUN npm run build

FROM nginx:alpine
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
"#;
        let df = parse(input).unwrap();
        assert_eq!(df.stages.len(), 2);

        let builder = &df.stages[0];
        assert_eq!(builder.from.image, "node:20-alpine");
        assert_eq!(builder.from.alias.as_deref(), Some("builder"));
        assert_eq!(builder.instructions.len(), 5);

        let runtime = &df.stages[1];
        assert_eq!(runtime.from.image, "nginx:alpine");
        assert!(runtime.from.alias.is_none());
        assert_eq!(runtime.instructions.len(), 3);
    }

    #[test]
    fn parses_line_continuations() {
        let input = "RUN apt-get update && \\\n    apt-get install -y \\\n    curl wget\n";
        let df = parse(input).unwrap();
        assert_eq!(df.stages.len(), 1);
        assert_eq!(df.stages[0].instructions.len(), 1);
        assert!(df.stages[0].instructions[0].args.contains("apt-get update"));
        assert!(df.stages[0].instructions[0].args.contains("curl wget"));
    }

    #[test]
    fn parses_from_with_platform() {
        let input = "FROM --platform=linux/amd64 ubuntu:22.04\nRUN echo hi\n";
        let df = parse(input).unwrap();
        assert_eq!(df.stages[0].from.platform.as_deref(), Some("linux/amd64"));
        assert_eq!(df.stages[0].from.image, "ubuntu:22.04");
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let input = "# This is a comment\n\nFROM alpine\n# inline\nRUN echo 1\n";
        let df = parse(input).unwrap();
        assert_eq!(df.stages.len(), 1);
        assert_eq!(df.stages[0].instructions.len(), 1);
    }

    #[test]
    fn tokenize_respects_quotes() {
        let tokens = tokenize(r#"cmd "arg with space" 'another arg'"#);
        assert_eq!(tokens, vec!["cmd", "arg with space", "another arg"]);
    }

    #[test]
    fn parse_from_with_alias() {
        let from = parse_from("ubuntu:22.04 AS base").unwrap();
        assert_eq!(from.image, "ubuntu:22.04");
        assert_eq!(from.alias.as_deref(), Some("base"));
    }

    #[test]
    fn rejects_empty_dockerfile() {
        let result = parse("# just a comment\n");
        assert!(result.is_err());
    }
}
