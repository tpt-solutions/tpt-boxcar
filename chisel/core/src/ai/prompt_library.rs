use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub system: String,
    pub user_template: String,
    pub expected_format: ResponseFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseFormat {
    Json,
    Markdown,
    PlainText,
}

pub struct PromptLibrary;

impl PromptLibrary {
    pub fn distillation_plan() -> PromptTemplate {
        PromptTemplate {
            name: "distillation_plan".to_string(),
            system: "You are a container image optimization expert. Analyze the provided \
                     image analysis data and create a distillation plan that minimizes \
                     image size while maintaining functionality. Return valid JSON."
                .to_string(),
            user_template: "Analyze this container image and create a distillation plan:\n\n\
                            Image: {image_name}:{image_tag}\n\
                            Size: {total_size} bytes\n\
                            Layers: {layer_count}\n\
                            Dependencies: {dependency_count}\n\n\
                            Provide a structured plan with steps, size reduction estimate, \
                            and risk assessment."
                .to_string(),
            expected_format: ResponseFormat::Json,
        }
    }

    pub fn wasm_migration_plan() -> PromptTemplate {
        PromptTemplate {
            name: "wasm_migration_plan".to_string(),
            system: "You are a WebAssembly migration specialist. Analyze the compatibility \
                     assessment and create a detailed migration plan. Return valid JSON."
                .to_string(),
            user_template: "Create a Wasm migration plan:\n\n\
                            Language: {language}\n\
                            Compatibility: {compatibility}\n\
                            Missing features: {missing_features}\n\
                            Required imports: {required_imports}\n\n\
                            Provide phases, effort estimates, and risk factors."
                .to_string(),
            expected_format: ResponseFormat::Json,
        }
    }

    pub fn security_audit() -> PromptTemplate {
        PromptTemplate {
            name: "security_audit".to_string(),
            system: "You are a container security auditor. Review the SBOM and CVE scan \
                     results, then provide a comprehensive security assessment. Return valid JSON."
                .to_string(),
            user_template: "Perform security audit:\n\n\
                            SBOM format: {sbom_format}\n\
                            Total dependencies: {total_deps}\n\
                            Vulnerabilities: {vuln_count}\n\
                            Critical: {critical}\n\
                            High: {high}\n\n\
                            Provide findings, risk level, and compliance notes."
                .to_string(),
            expected_format: ResponseFormat::Json,
        }
    }

    pub fn analyze_usage_pattern() -> PromptTemplate {
        PromptTemplate {
            name: "analyze_usage_pattern".to_string(),
            system: "You are a runtime analysis expert. Examine the trace data to identify \
                     actual usage patterns and recommend optimizations. Return valid JSON."
                .to_string(),
            user_template: "Analyze runtime usage:\n\n\
                            Entry point: {entrypoint}\n\
                            CPU seconds: {cpu_seconds}\n\
                            Peak memory: {memory_peak} bytes\n\
                            Syscalls: {syscall_count}\n\
                            Network endpoints: {network_count}\n\
                            File paths: {file_count}\n\n\
                            Identify unused code and recommend optimizations."
                .to_string(),
            expected_format: ResponseFormat::Json,
        }
    }

    pub fn fill_template(template: &PromptTemplate, variables: &HashMap<String, String>) -> String {
        let mut filled = template.user_template.clone();
        for (key, value) in variables {
            let placeholder = format!("{{{key}}}");
            filled = filled.replace(&placeholder, value);
        }
        filled
    }
}
