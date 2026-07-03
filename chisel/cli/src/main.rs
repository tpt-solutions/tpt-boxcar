use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tpt_chisel_core::analyzer::Analyzer;
use tpt_chisel_core::distiller::Distiller;

mod config;

#[derive(Parser)]
#[command(
    name = "chisel",
    about = "TPT Chisel — image analysis, distillation, and Wasm migration",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run two-phase analysis (runtime trace + Wasm migration feasibility) on an image
    Analyze {
        /// Path to an extracted OCI image directory
        image: PathBuf,
        /// Print raw JSON instead of a human-readable summary
        #[arg(long)]
        json: bool,
    },
    /// Analyze then distill an image into a minimal, Wasm-migration-aware image
    Distill {
        /// Path to an extracted OCI image directory
        image: PathBuf,
        /// Also print a generated Dockerfile for the distilled image
        #[arg(long)]
        dockerfile: bool,
        /// Also print an SBOM in the given format (only "spdx" is supported; the
        /// distilled image's embedded SBOM is already CycloneDX by default)
        #[arg(long, value_name = "FORMAT")]
        sbom: Option<String>,
        /// Print raw JSON instead of a human-readable summary
        #[arg(long)]
        json: bool,
    },
    /// Analyze, distill, then generate an AI-assisted Wasm migration plan
    Migrate {
        /// Path to an extracted OCI image directory
        image: PathBuf,
        /// Path to a chisel.yaml AI config (see chisel/examples/chisel.yaml)
        #[arg(long, default_value = "chisel.yaml")]
        config: PathBuf,
        /// Override the config file's ai.backend ("local" or "cloud")
        #[arg(long)]
        ai: Option<String>,
    },
    /// Distill an image then generate an AI-assisted security audit
    Audit {
        /// Path to an extracted OCI image directory
        image: PathBuf,
        /// Path to a chisel.yaml AI config (see chisel/examples/chisel.yaml)
        #[arg(long, default_value = "chisel.yaml")]
        config: PathBuf,
        /// Override the config file's ai.backend ("local" or "cloud")
        #[arg(long)]
        ai: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze { image, json } => cmd_analyze(&image, json).await,
        Commands::Distill {
            image,
            dockerfile,
            sbom,
            json,
        } => cmd_distill(&image, dockerfile, sbom.as_deref(), json).await,
        Commands::Migrate { image, config, ai } => {
            cmd_migrate(&image, &config, ai.as_deref()).await
        }
        Commands::Audit { image, config, ai } => cmd_audit(&image, &config, ai.as_deref()).await,
    }
}

async fn cmd_analyze(image: &PathBuf, json: bool) -> anyhow::Result<()> {
    let analysis = Analyzer::new(image).analyze().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
        return Ok(());
    }

    println!("Image:        {}:{}", analysis.phase1.image.name, analysis.phase1.image.tag);
    println!("Size:         {} bytes across {} layers", analysis.phase1.image.total_size_bytes, analysis.phase1.image.layer_count);
    println!("Language:     {:?}", analysis.phase2.detected_language);
    println!("Wasm target:  {}", analysis.phase2.suggested_target);
    println!("Wasm compat:  {:?}", analysis.phase2.wasm_compatibility);
    if !analysis.phase2.compilation_hints.is_empty() {
        println!("Hints:");
        for hint in &analysis.phase2.compilation_hints {
            println!("  - {hint}");
        }
    }
    Ok(())
}

async fn cmd_distill(
    image: &PathBuf,
    dockerfile: bool,
    sbom: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let analysis = Analyzer::new(image).analyze().await?;
    let distiller = Distiller::new();
    let distilled = distiller.distill(&analysis)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&distilled)?);
    } else {
        println!("Distilled:    {}:{}", distilled.name, distilled.tag);
        println!("Base image:   {}", distilled.base_image);
        println!(
            "Size:         {} bytes ({:.1}% of original)",
            distilled.total_size_bytes,
            distilled.distillation_ratio * 100.0
        );
        println!(
            "CVE scan:     {} findings ({} critical, {} high)",
            distilled.cve_scan.vulnerabilities_found, distilled.cve_scan.critical, distilled.cve_scan.high
        );
        if let Some(path) = &distilled.wasm_migration_path {
            println!(
                "Wasm path:    -> {} (~{:.0}% size reduction)",
                path.target,
                path.estimated_size_reduction * 100.0
            );
        }
    }

    if dockerfile {
        println!("\n--- Dockerfile ---");
        println!("{}", distiller.generate_dockerfile(&distilled));
    }

    if let Some(format) = sbom {
        if format.eq_ignore_ascii_case("spdx") {
            let spdx = distiller.generate_spdx_sbom(&analysis.phase1.dependencies)?;
            println!("\n--- SBOM (SPDX) ---");
            println!("{}", serde_json::to_string_pretty(&spdx)?);
        } else {
            anyhow::bail!("Unsupported SBOM format: {format} (only \"spdx\" is supported; distill's own output already embeds a CycloneDX SBOM)");
        }
    }

    Ok(())
}

async fn cmd_migrate(image: &PathBuf, config_path: &PathBuf, ai_override: Option<&str>) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let orchestrator = config::build_orchestrator(&config, ai_override)?;

    let analysis = Analyzer::new(image).analyze().await?;
    let plan = orchestrator.generate_migration_plan(&analysis).await?;

    println!("Source language: {}", plan.source_language);
    println!("Target platform: {}", plan.target_platform);
    println!("Estimated effort: {}", plan.estimated_effort);
    if !plan.risk_factors.is_empty() {
        println!("Risk factors:");
        for risk in &plan.risk_factors {
            println!("  - {risk}");
        }
    }
    println!("Phases:");
    for phase in &plan.phases {
        println!(
            "  {} (~{:.1}h){}",
            phase.name,
            phase.estimated_hours,
            if phase.dependencies.is_empty() {
                String::new()
            } else {
                format!(" [depends on: {}]", phase.dependencies.join(", "))
            }
        );
        for task in &phase.tasks {
            println!("    - {task}");
        }
    }
    Ok(())
}

async fn cmd_audit(image: &PathBuf, config_path: &PathBuf, ai_override: Option<&str>) -> anyhow::Result<()> {
    let config = config::load(config_path)?;
    let orchestrator = config::build_orchestrator(&config, ai_override)?;

    let analysis = Analyzer::new(image).analyze().await?;
    let distilled = Distiller::new().distill(&analysis)?;
    let audit = orchestrator.generate_security_audit(&distilled).await?;

    println!("Overall risk: {}", audit.overall_risk);
    if !audit.findings.is_empty() {
        println!("Findings:");
        for finding in &audit.findings {
            println!(
                "  [{}] {} — {}",
                finding.severity, finding.category, finding.description
            );
            println!("    remediation: {}", finding.remediation);
        }
    }
    if !audit.compliance_notes.is_empty() {
        println!("Compliance notes:");
        for note in &audit.compliance_notes {
            println!("  - {note}");
        }
    }
    Ok(())
}
