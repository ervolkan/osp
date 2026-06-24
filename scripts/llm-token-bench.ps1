#
# OSP Real LLM Token Benchmark — OpenAI GPT-4o
#
# iki yaklaşımda prompt gönderir, gerçek token sayılarını API'den alır:
# 1. OSP coordinate prompt (155 chars JSON)
# 2. Raw source file dump (2-hop context)
#
# Usage: pwsh scripts/llm-token-bench.ps1
#

$apiKeyPath = "docs/llm-apikey.md"
$apiKey = (Get-Content $apiKeyPath -Raw).Trim()
$apiUrl = "https://api.openai.com/v1/chat/completions"
$model = "gpt-4o-mini"

$headers = @{
    "Authorization" = "Bearer $apiKey"
    "Content-Type" = "application/json"
}

# ── 1. OSP Coordinate Prompt ──
$ospSystemPrompt = @"
You are an OSP (Ontological Space Protocol) agent. You receive a typed epistemic projection packet containing module coordinates in a 5-dimensional architectural space. Respond with a DeltaProposal JSON describing structural changes only (no positions — the engine computes those).

Coordinate axes: x=coupling, y=cohesion, z=instability, w=entropy, v=witness-depth.
Vision: x≤0.30, y≥0.70, z≤0.50.
Output format: JSON with fields: new_nodes, new_edges, modified_entities, reasoning.
"@

$ospUserPrompt = @"
OspPrompt:
{
  "space_slice": {
    "nodes": [
      {"id": 0, "kind": "Module", "name": "coords.rs", "position": {"x": 0.15, "y": 0.85, "z": 0.40, "w": 0.60, "v": 0.70}},
      {"id": 1, "kind": "Module", "name": "witness.rs", "position": {"x": 0.25, "y": 0.72, "z": 0.45, "w": 0.58, "v": 0.80}},
      {"id": 2, "kind": "Module", "name": "engine.rs", "position": {"x": 0.35, "y": 0.68, "z": 0.50, "w": 0.62, "v": 0.65}},
      {"id": 3, "kind": "Module", "name": "space.rs", "position": {"x": 0.10, "y": 0.90, "z": 0.35, "w": 0.55, "v": 0.60}}
    ],
    "edges": [
      {"from": 2, "to": 0, "kind": "Imports"},
      {"from": 2, "to": 1, "kind": "Imports"},
      {"from": 2, "to": 3, "kind": "Imports"}
    ]
  },
  "vision": {"x": 0.30, "y": 0.70, "z": 0.50, "w": 0.60, "v": 0.70},
  "rules": ["no_self_import", "no_duplicate_node"],
  "intent": "Add a new logging module that imports coords.rs for position logging",
  "output_contract": "Respond with DeltaProposal JSON: new_nodes, new_edges, reasoning"
}

Produce a DeltaProposal for this intent.
"@

# ── 2. Raw Source Dump Prompt ──
$rawSystemPrompt = "You are a coding assistant. The user will show you source files and ask you to add a feature."

$coordsRs = Get-Content "crates/osp-core/src/coords.rs" -Raw -ErrorAction SilentlyContinue
if (-not $coordsRs) { $coordsRs = "// coords.rs - coordinate system types" }
$engineRs = Get-Content "crates/osp-core/src/engine.rs" -Raw -ErrorAction SilentlyContinue
if (-not $engineRs) { $engineRs = "// engine.rs - space engine" }

$rawUserPrompt = @"
Here are source files from the project:

=== coords.rs ===
$($coordsRs.Substring(0, [Math]::Min(2000, $coordsRs.Length)))

=== engine.rs ===
$($engineRs.Substring(0, [Math]::Min(2000, $engineRs.Length)))

Task: Add a new logging module that imports coords.rs for position logging. Write the new module code.
"@

# ── Call API ──
function Call-OpenAI($system, $user, $label) {
    $body = @{
        model = $model
        messages = @(
            @{ role = "system"; content = $system }
            @{ role = "user"; content = $user }
        )
        max_tokens = 500
        temperature = 0.3
    } | ConvertTo-Json -Depth 5

    Write-Host "`n=== Calling OpenAI ($label) ===" -ForegroundColor Cyan
    try {
        $response = Invoke-RestMethod -Uri $apiUrl -Method POST -Headers $headers -Body $body -ContentType "application/json"
        $usage = $response.usage
        Write-Host "  Prompt tokens:     $($usage.prompt_tokens)" -ForegroundColor Green
        Write-Host "  Completion tokens: $($usage.completion_tokens)" -ForegroundColor Green
        Write-Host "  Total tokens:      $($usage.total_tokens)" -ForegroundColor Green
        Write-Host "  Response preview:  $($response.choices[0].message.content.Substring(0, [Math]::Min(200, $response.choices[0].message.content.Length)))..." -ForegroundColor Gray
        return @{
            prompt_tokens = $usage.prompt_tokens
            completion_tokens = $usage.completion_tokens
            total_tokens = $usage.total_tokens
            response = $response.choices[0].message.content
        }
    } catch {
        Write-Host "  ERROR: $($_.Exception.Message)" -ForegroundColor Red
        return $null
    }
}

Write-Host "========================================" -ForegroundColor Yellow
Write-Host "  OSP Real LLM Token Benchmark" -ForegroundColor Yellow
Write-Host "  Model: $model" -ForegroundColor Yellow
Write-Host "========================================" -ForegroundColor Yellow

# OSP prompt chars
$ospChars = ($ospSystemPrompt.Length + $ospUserPrompt.Length)
$rawChars = ($rawSystemPrompt.Length + $rawUserPrompt.Length)
Write-Host "`nInput sizes (chars):"
Write-Host "  OSP prompt:   $ospChars chars"
Write-Host "  Raw dump:     $rawChars chars"
Write-Host "  Ratio:        $([math]::Round($rawChars / $ospChars, 1))x larger"

# Call 1: OSP
$ospResult = Call-OpenAI $ospSystemPrompt $ospUserPrompt "OSP Coordinate Prompt"

# Call 2: Raw
$rawResult = Call-OpenAI $rawSystemPrompt $rawUserPrompt "Raw Source Dump"

# ── Summary ──
if ($ospResult -and $rawResult) {
    Write-Host "`n========================================" -ForegroundColor Yellow
    Write-Host "  RESULTS" -ForegroundColor Yellow
    Write-Host "========================================" -ForegroundColor Yellow

    Write-Host "`nPrompt Token Comparison (REAL tiktoken):"
    Write-Host "  OSP coordinate:   $($ospResult.prompt_tokens) tokens"
    Write-Host "  Raw source dump:  $($rawResult.prompt_tokens) tokens"
    $ratio = [math]::Round($rawResult.prompt_tokens / $ospResult.prompt_tokens, 1)
    $savings = [math]::Round((1 - $ospResult.prompt_tokens / $rawResult.prompt_tokens) * 100, 1)
    Write-Host "  Ratio:            1:${ratio} (OSP is ${ratio}x smaller)"
    Write-Host "  Savings:          ${savings}%"

    Write-Host "`nCompletion Token Comparison:"
    Write-Host "  OSP response:     $($ospResult.completion_tokens) tokens"
    Write-Host "  Raw response:     $($rawResult.completion_tokens) tokens"

    Write-Host "`nTotal Token Comparison:"
    Write-Host "  OSP total:        $($ospResult.total_tokens) tokens"
    Write-Host "  Raw total:        $($rawResult.total_tokens) tokens"

    # Cost estimate (gpt-4o-mini: $0.150/1M input, $0.600/1M output)
    $ospCost = ($ospResult.prompt_tokens * 0.150 + $ospResult.completion_tokens * 0.600) / 1000000
    $rawCost = ($rawResult.prompt_tokens * 0.150 + $rawResult.completion_tokens * 0.600) / 1000000
    Write-Host "`nCost Estimate (gpt-4o-mini pricing):"
    Write-Host "  OSP cost:         `$$([math]::Round($ospCost, 6))"
    Write-Host "  Raw cost:         `$$([math]::Round($rawCost, 6))"

    # Save results
    $results = @{
        model = $model
        timestamp = (Get-Date -Format "o")
        osp = @{
            prompt_tokens = $ospResult.prompt_tokens
            completion_tokens = $ospResult.completion_tokens
            total_tokens = $ospResult.total_tokens
            input_chars = $ospChars
        }
        raw = @{
            prompt_tokens = $rawResult.prompt_tokens
            completion_tokens = $rawResult.completion_tokens
            total_tokens = $rawResult.total_tokens
            input_chars = $rawChars
        }
        ratio = $ratio
        savings_pct = $savings
    }
    $results | ConvertTo-Json -Depth 4 | Set-Content "docs/usage-llm-benchmark.json"
    Write-Host "`nResults saved to docs/usage-llm-benchmark.json" -ForegroundColor Green
}
