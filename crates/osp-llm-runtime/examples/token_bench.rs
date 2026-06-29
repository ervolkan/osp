//! OSP vs raw-dump token benchmark (RQ5 / §7.8).
//!
//! Drop-in Rust replacement for `scripts/llm-token-bench.ps1`. Reads the API
//! key from `OPENAI_API_KEY`, sends the same OSP coordinate prompt and raw
//! 2-hop source dump as the PowerShell baseline, and prints the real
//! `prompt_tokens` / `completion_tokens` reported by the model.
//!
//! Usage:
//!     $env:OPENAI_API_KEY = "sk-..."
//!     cargo run -p osp-llm-runtime --example token_bench
//!
//! Optional args (positional):
//!     1 = model override (default: gpt-4o-mini)

use osp_core::agent::{OspPrompt, OutputContract};
use osp_core::space::TimeLayer;
use osp_core::vision::VisionVector;

use osp_llm_runtime::{osp_system_prompt, raw_dump_user_prompt, raw_system_prompt};
use osp_llm_runtime::{CompletionRequest, Runtime, RuntimeConfig};

const USAGE: &str = "usage: token_bench [model]\n(env OPENAI_API_KEY required)";

fn read_snippet(path: &str, cap: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) if s.len() > cap => format!("{}…", &s[..cap]),
        Ok(s) => s,
        Err(_) => format!("// {path} (not found)"),
    }
}

fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let cfg = RuntimeConfig {
        model: model.clone(),
        ..RuntimeConfig::default()
            .with_env_api_key()
            .map_err(|e| anyhow::anyhow!("{USAGE}: {e}"))?
    };
    let runtime = Runtime::new(cfg)?;

    println!("========================================");
    println!("  OSP Real LLM Token Benchmark");
    println!("  Model: {model}");
    println!("========================================");

    // OSP coordinate prompt — the canonical 5-axis packet.
    let osp_prompt = OspPrompt {
        vision: VisionVector::default(),
        time_ref: TimeLayer::default(),
        permissions: Default::default(),
        output_contract: OutputContract::default(),
    };
    let osp_req = CompletionRequest {
        system: osp_system_prompt().to_string(),
        user: osp_llm_runtime::osp_user_prompt(&osp_prompt),
    };

    // Raw 2-hop dump — coords.rs + engine.rs (same files as the PS baseline).
    let coords = read_snippet("crates/osp-core/src/coords.rs", 2000);
    let engine = read_snippet("crates/osp-core/src/engine.rs", 2000);
    let raw_req = CompletionRequest {
        system: raw_system_prompt().to_string(),
        user: raw_dump_user_prompt(
            &[
                ("coords.rs", &coords),
                ("engine.rs", &engine),
            ],
            "Add a new logging module that imports coords.rs for position logging. Write the new module code.",
        ),
    };

    println!("\nInput sizes (chars):");
    println!("  OSP prompt:   {} chars", osp_req.input_chars());
    println!("  Raw dump:     {} chars", raw_req.input_chars());
    let char_ratio = raw_req.input_chars() as f64 / osp_req.input_chars().max(1) as f64;
    println!("  Ratio:        {:.1}x larger", char_ratio);

    println!("\n=== Calling OpenAI (OSP Coordinate Prompt) ===");
    let osp_completion = match runtime.complete_raw(&osp_req) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  OSP call failed: {e}");
            return Err(anyhow::anyhow!("osp call failed: {e}"));
        }
    };
    println!(
        "  prompt_tokens:     {}",
        osp_completion.usage.prompt_tokens
    );
    println!(
        "  completion_tokens: {}",
        osp_completion.usage.completion_tokens
    );
    println!("  total_tokens:      {}", osp_completion.usage.total_tokens);

    println!("\n=== Calling OpenAI (Raw Source Dump) ===");
    let raw_completion = match runtime.complete_raw(&raw_req) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  raw call failed: {e}");
            return Err(anyhow::anyhow!("raw call failed: {e}"));
        }
    };
    println!(
        "  prompt_tokens:     {}",
        raw_completion.usage.prompt_tokens
    );
    println!(
        "  completion_tokens: {}",
        raw_completion.usage.completion_tokens
    );
    println!("  total_tokens:      {}", raw_completion.usage.total_tokens);

    let ratio = raw_completion.usage.prompt_tokens as f64
        / osp_completion.usage.prompt_tokens.max(1) as f64;
    let savings = 100.0
        * (1.0
            - osp_completion.usage.prompt_tokens as f64
                / raw_completion.usage.prompt_tokens as f64);
    println!("\n========================================");
    println!("  RESULTS (real tokenizer)");
    println!("========================================");
    println!(
        "  OSP  prompt: {} tokens",
        osp_completion.usage.prompt_tokens
    );
    println!(
        "  Raw  prompt: {} tokens",
        raw_completion.usage.prompt_tokens
    );
    println!("  Ratio:       1:{ratio:.1} (OSP {ratio:.1}x smaller)");
    println!("  Savings:     {savings:.1}%");

    // gpt-4o-mini pricing: $0.150/1M input, $0.600/1M output
    let osp_cost = (osp_completion.usage.prompt_tokens as f64 * 0.150
        + osp_completion.usage.completion_tokens as f64 * 0.600)
        / 1_000_000.0;
    let raw_cost = (raw_completion.usage.prompt_tokens as f64 * 0.150
        + raw_completion.usage.completion_tokens as f64 * 0.600)
        / 1_000_000.0;
    println!("\nCost (gpt-4o-mini):");
    println!("  OSP:  ${osp_cost:.6}");
    println!("  Raw:  ${raw_cost:.6}");

    // Persist JSON results (same shape as docs/usage-llm-benchmark.json).
    let results = serde_json::json!({
        "model": model,
        "timestamp": chrono_now(),
        "osp": {
            "prompt_tokens": osp_completion.usage.prompt_tokens,
            "completion_tokens": osp_completion.usage.completion_tokens,
            "total_tokens": osp_completion.usage.total_tokens,
            "input_chars": osp_req.input_chars(),
        },
        "raw": {
            "prompt_tokens": raw_completion.usage.prompt_tokens,
            "completion_tokens": raw_completion.usage.completion_tokens,
            "total_tokens": raw_completion.usage.total_tokens,
            "input_chars": raw_req.input_chars(),
        },
        "ratio": (ratio * 10.0).round() / 10.0,
        "savings_pct": (savings * 10.0).round() / 10.0,
    });
    let out = "docs/usage-llm-benchmark.json";
    std::fs::write(out, serde_json::to_string_pretty(&results)?)?;
    println!("\nResults saved to {out}");

    Ok(())
}

fn chrono_now() -> String {
    // Avoid pulling chrono for a single timestamp; RFC3339-ish via std.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("unix:{secs}")
}
