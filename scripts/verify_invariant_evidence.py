#!/usr/bin/env python3
"""Verify the Paper 3 invariant-evidence-matrix.json against the live codebase.

For each evidence entry:
  - rust_test:        cargo test -p <pkg> [--lib|--test <target>] <test_name> -- --exact
                      must select exactly 1 test, run it, pass, and not be ignored.
  - trybuild_fixture: fixture file exists, orchestrator registers it via compile_fail("..."),
                      and the orchestration test runs and passes.
  - architecture_guard: treated as rust_test with target_kind=integration.
  - type_shape:       no executable test; only checked for structural presence of the
                      implementation_ref path.
  - gap:              skipped (flagged for manual resolution before v1.4 freeze).

Also cross-checks:
  - run-metadata.md vs run-metadata.json critical fields (frozen/current counts, hashes).
  - MANIFEST.json artifact sha256 vs git blob sha256 (canonical, CRLF-safe).

Exit code 0 = all verifications passed; non-zero = one or more failures (CI signal).

Windows + CI compatible: uses only subprocess, os, sys, json, re, hashlib. No shell
redirection; all cargo/git output captured via subprocess.check_output/capture_output.
"""

import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
MATRIX_PATH = REPO_ROOT / "docs" / "paper3-notes" / "evidence" / "invariant-evidence-matrix.json"
MANIFEST_PATH = REPO_ROOT / "docs" / "paper3-notes" / "evidence-pack" / "MANIFEST.json"
RUN_METADATA_JSON = REPO_ROOT / "docs" / "paper3-notes" / "evidence" / "run-metadata.json"
RUN_METADATA_MD = REPO_ROOT / "docs" / "paper3-notes" / "evidence" / "run-metadata.md"


class Result:
    """Verification result. `category` distinguishes how the entry was verified:
    - 'verified'   : cargo test / trybuild / architecture guard ran green
    - 'manually_asserted' : type-shape — path + symbol presence checked, but the
                     semantic enforcement (sealed module / write path / newtype)
                     requires manual inspection (no compile-fail fixture)
    - 'skipped'    : gap entry — flagged for manual resolution, not counted as evidence
    """

    def __init__(self, ok: bool, detail: str = "", category: str = "verified"):
        self.ok = ok
        self.detail = detail
        self.category = category

    def __bool__(self) -> bool:
        return self.ok


def run_cargo_test(package: str, target_kind: str, target_name, test_name: str,
                   expected_selected: int = 1) -> Result:
    """Run a single rust test via `cargo test ... -- --exact` and verify exactly one
    test was selected and passed."""
    cmd = ["cargo", "test", "-p", package]
    if target_kind == "lib":
        cmd.append("--lib")
    elif target_kind == "integration":
        if not target_name:
            return Result(False, "integration target requires target_name")
        cmd.extend(["--test", target_name])
    else:
        return Result(False, f"unknown target_kind: {target_kind}")
    cmd.append(test_name)
    cmd.extend(["--", "--exact"])

    try:
        proc = subprocess.run(
            cmd,
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=600,
        )
    except subprocess.TimeoutExpired:
        return Result(False, f"timeout running: {' '.join(cmd)}")
    except FileNotFoundError:
        return Result(False, "cargo not found on PATH")

    out = proc.stdout + proc.stderr

    if proc.returncode != 0:
        return Result(False, f"cargo test exit {proc.returncode}; cmd={' '.join(cmd)}\n{out[-800:]}")

    # Parse the test result line: "test result: ok. 1 passed; 0 failed; 0 ignored; ..."
    m = re.search(
        r"test result: (ok|FAILED)\.\s+(\d+) passed;\s+(\d+) failed;\s+(\d+) ignored;",
        out,
    )
    if not m:
        return Result(False, f"could not parse test result line in output:\n{out[-600:]}")
    status, passed, failed, ignored = m.group(1), int(m.group(2)), int(m.group(3)), int(m.group(4))

    if status != "ok":
        return Result(False, f"test result FAILED: {passed}p/{failed}f/{ignored}i")
    if failed != 0:
        return Result(False, f"{failed} tests failed")
    if passed != expected_selected:
        return Result(
            False,
            f"expected {expected_selected} selected/passed but got {passed} "
            f"(typo in test_name? filtered to 0 and silently passed?)",
        )
    if ignored != 0:
        return Result(False, f"{ignored} tests ignored (test not actually run)")
    return Result(True, f"{passed} passed, 0 failed, 0 ignored")


def verify_trybuild_fixture(fixture_rel: str, orchestrator_rel: str,
                            orchestrator_test: str) -> Result:
    """Verify a trybuild compile-fail fixture:
       (1) fixture .rs file exists,
       (2) orchestrator registers it via compile_fail("<fixture>"),
       (3) orchestration test runs and passes (calls the orchestration test by exact name).
    """
    fixture_path = REPO_ROOT / "crates" / "osp-core" / fixture_rel
    if not fixture_path.exists():
        return Result(False, f"fixture file not found: {fixture_rel}")

    orch_path = REPO_ROOT / "crates" / "osp-core" / orchestrator_rel
    if not orch_path.exists():
        return Result(False, f"orchestrator file not found: {orchestrator_rel}")
    orch_src = orch_path.read_text(encoding="utf-8")

    # Check registration: the orchestrator must call compile_fail with the fixture path.
    # trybuild API: t.compile_fail("tests/compile_fail/<name>.rs")
    registration_needle = f'compile_fail("{fixture_rel}")'
    # Also tolerate compile_fail('...') single-quote form.
    registration_needle_alt = f"compile_fail('{fixture_rel}')"
    if registration_needle not in orch_src and registration_needle_alt not in orch_src:
        # Fall back to checking just the basename (some orchestrators use shorter paths).
        basename = Path(fixture_rel).name
        if f'compile_fail("' not in orch_src or basename not in orch_src:
            return Result(
                False,
                f"fixture {fixture_rel} not registered in orchestrator {orchestrator_rel}",
            )

    # Run the orchestration test by exact name to confirm the trybuild run passes.
    return run_cargo_test(
        package="osp-core",
        target_kind="integration",
        target_name=Path(orchestrator_rel).stem,
        test_name=orchestrator_test,
        expected_selected=1,
    )


def git_blob_sha256(repo_path: Path, rel_path: str) -> str:
    """Compute sha256 of a git blob at HEAD (canonical; CRLF/encoding-safe)."""
    blob = subprocess.check_output(
        ["git", "cat-file", "-p", f"HEAD:{rel_path.replace(os.sep, '/')}"],
        cwd=repo_path,
    )
    return hashlib.sha256(blob).hexdigest()


def verify_manifest_hashes(manifest: dict) -> list:
    """Verify each artifact sha256 in MANIFEST.json matches the git blob sha256."""
    results = []
    for entry in manifest.get("files", []):
        fname = entry.get("file")
        recorded = entry.get("sha256")
        if not fname or not recorded:
            continue
        rel = f"docs/paper3-notes/evidence/{fname}"
        try:
            actual = git_blob_sha256(REPO_ROOT, rel)
        except subprocess.CalledProcessError as e:
            results.append((fname, False, f"git cat-file failed: {e}"))
            continue
        if actual != recorded:
            results.append(
                (fname, False, f"MANIFEST sha {recorded} != git blob sha {actual}")
            )
        else:
            results.append((fname, True, actual))
    return results


def verify_type_shape(entry: dict) -> Result:
    """type_shape: path + symbols varlığını kontrol eder. Enforcement'ın anlamı
    (sealed module / unconditional write path / non-empty newtype) manuel inceleme
    gerektirir; compile-fail fixture yok. Bu yüzden 'manually_asserted' kategorisidir."""
    path = entry.get("path")
    symbols = entry.get("symbols", [])
    if not path:
        return Result(
            False,
            "type_shape entry missing 'path' field (review P1 #2: path+symbols required)",
            category="manually_asserted",
        )
    full_path = REPO_ROOT / path
    if not full_path.exists():
        return Result(
            False,
            f"type_shape path not found: {path}",
            category="manually_asserted",
        )
    src = full_path.read_text(encoding="utf-8")
    missing_symbols = [s for s in symbols if s not in src]
    if missing_symbols:
        return Result(
            False,
            f"type_shape symbols not found in {path}: {missing_symbols}",
            category="manually_asserted",
        )
    return Result(
        True,
        f"path + {len(symbols)} symbol(s) present ({', '.join(symbols)}); enforcement "
        "semantics manually asserted (no compile-fail fixture)",
        category="manually_asserted",
    )


def verify_entry(entry: dict) -> Result:
    kind = entry.get("evidence_kind")
    if kind == "gap":
        return Result(True, "skipped (gap — flagged for manual resolution)", category="skipped")
    if kind == "type_shape":
        return verify_type_shape(entry)
    if kind == "architecture_guard":
        return run_cargo_test(
            package=entry["package"],
            target_kind=entry["target_kind"],
            target_name=entry["target_name"],
            test_name=entry["test_name"],
            expected_selected=entry.get("expected_selected", 1),
        )
    if kind == "rust_test":
        return run_cargo_test(
            package=entry["package"],
            target_kind=entry["target_kind"],
            target_name=entry.get("target_name"),
            test_name=entry["test_name"],
            expected_selected=entry.get("expected_selected", 1),
        )
    if kind in ("rust_test_matrix",):
        # Verify the primary test; the error_path_tests list is informational.
        return run_cargo_test(
            package=entry["package"],
            target_kind=entry["target_kind"],
            target_name=entry.get("target_name"),
            test_name=entry["primary_test"],
            expected_selected=entry.get("expected_selected", 1),
        )
    if kind == "trybuild_fixture":
        fixtures = entry.get("fixtures", [])
        orchestrator = entry["orchestrator"]
        orchestrator_test = entry["orchestrator_test"]
        for fixture in fixtures:
            r = verify_trybuild_fixture(fixture, orchestrator, orchestrator_test)
            if not r:
                return Result(False, f"fixture {fixture}: {r.detail}")
        return Result(True, f"{len(fixtures)} fixtures verified via {orchestrator_test}")
    return Result(False, f"unknown evidence_kind: {kind}")


def verify_run_metadata_crosscheck() -> Result:
    """Cross-check run-metadata.md vs run-metadata.json critical counts.
    Line-bazlı kontrol (review P2 #3): her sayının geçtiği satırda ilgili keyword
    de olmalı; rastgele metin varlığı yeterli değildir, ama çok katı pattern de değil."""
    if not RUN_METADATA_JSON.exists():
        return Result(False, "run-metadata.json missing")
    if not RUN_METADATA_MD.exists():
        return Result(False, "run-metadata.md missing")
    j = json.loads(RUN_METADATA_JSON.read_text(encoding="utf-8"))
    md_lines = RUN_METADATA_MD.read_text(encoding="utf-8").splitlines()

    cur = j.get("current_protocol", {})
    # Her (label, json_value, line_keywords): sayıyı içeren en az bir satırda
    # keyword'lerden en az biri de geçmeli. Line-bazlı proximity.
    checks = [
        (
            "core_binding_invariants",
            cur.get("core_binding_invariants"),
            ["invariant", "binding", "INV-C"],
        ),
        (
            "cumulative_compile_fail",
            cur.get("compile_fail", {}).get("cumulative_workspace"),
            ["compile-fail", "compile_fail", "cumulative"],
        ),
        (
            "paper3_specific_compile_fail",
            cur.get("compile_fail", {}).get("paper3_specific"),
            ["Paper-3-specific", "Paper 3", "paper3"],
        ),
        (
            "osp_core_tests",
            cur.get("osp_core_tests"),
            ["osp-core", "osp_core", "lib"],
        ),
    ]
    mismatches = []
    for label, value, keywords in checks:
        if value is None:
            mismatches.append(f"{label}: json value missing")
            continue
        value_str = str(value)
        # Sayıyı içeren satırları bul; en az birinde keyword'lerden biri geçmeli.
        candidate_lines = [ln for ln in md_lines if value_str in ln]
        if not candidate_lines:
            mismatches.append(f"{label}={value_str}: value not found in any line of run-metadata.md")
            continue
        keyword_match = any(
            any(kw in ln for kw in keywords) for ln in candidate_lines
        )
        if not keyword_match:
            mismatches.append(
                f"{label}={value_str}: found in line(s) but none contains keywords {keywords}"
            )
    if mismatches:
        return Result(False, "; ".join(mismatches))
    return Result(True, "run-metadata.md ↔ run-metadata.json critical counts aligned (line-based)")


def verify_summary_consistency(matrix: dict, verified: int, manually_asserted: int, skipped: int, failed: int) -> Result:
    """summary.total_evidence_entries == len(evidence_entries) ve
    verified + manually_asserted + skipped + failed == len(entries) (review P2 #1, P2 #3)."""
    entries = matrix.get("evidence_entries", [])
    actual_count = len(entries)
    summary = matrix.get("summary", {})
    declared_total = summary.get("total_evidence_entries")
    declared_verified = summary.get("verified_entries")
    declared_manual = summary.get("manually_asserted_entries")

    problems = []
    if declared_total != actual_count:
        problems.append(
            f"summary.total_evidence_entries ({declared_total}) != len(evidence_entries) ({actual_count})"
        )
    runtime_total = verified + manually_asserted + skipped + failed
    if runtime_total != actual_count:
        problems.append(
            f"verified({verified}) + manually_asserted({manually_asserted}) + skipped({skipped}) "
            f"+ failed({failed}) = {runtime_total} != len(entries) ({actual_count})"
        )
    if declared_verified is not None and declared_verified != verified:
        problems.append(
            f"summary.verified_entries ({declared_verified}) != runtime verified ({verified})"
        )
    if declared_manual is not None and declared_manual != manually_asserted:
        problems.append(
            f"summary.manually_asserted_entries ({declared_manual}) != runtime manually_asserted ({manually_asserted})"
        )
    if problems:
        return Result(False, "; ".join(problems))
    return Result(True, f"summary consistent: {actual_count} entries, {verified} verified, {manually_asserted} manually asserted")


def main() -> int:
    if not MATRIX_PATH.exists():
        print(f"FATAL: matrix not found at {MATRIX_PATH}", file=sys.stderr)
        return 2

    matrix = json.loads(MATRIX_PATH.read_text(encoding="utf-8"))
    entries = matrix.get("evidence_entries", [])

    failures = []
    verified = 0
    manually_asserted = 0
    skipped = 0

    print(f"Verifying {len(entries)} evidence entries...")
    for entry in entries:
        inv = entry.get("invariant", "?")
        eid = entry.get("evidence_id", "?")
        kind = entry.get("evidence_kind", "?")
        r = verify_entry(entry)
        if not r:
            failures.append((inv, eid, r.detail))
            status_str = "FAIL"
        elif r.category == "skipped":
            skipped += 1
            status_str = "SKIP"
        elif r.category == "manually_asserted":
            manually_asserted += 1
            status_str = "MANUAL"
        else:
            verified += 1
            status_str = "ok"
        print(f"  [{status_str}] {eid:<18} {inv:<22} ({kind})")
        if r.detail and status_str in ("ok", "MANUAL"):
            print(f"          {r.detail}")

    print()
    print(f"verified (cargo test/trybuild/guard): {verified}")
    print(f"manually asserted (type-shape):       {manually_asserted}")
    print(f"skipped (gap):                        {skipped}")
    print(f"failed:                               {len(failures)}")

    # Summary consistency: total == len(entries) ve counts tutarlı (review P2 #1).
    print()
    print("Matrix summary consistency check...")
    sr = verify_summary_consistency(matrix, verified, manually_asserted, skipped, len(failures))
    if sr:
        print(f"  [ok] {sr.detail}")
    else:
        print(f"  [FAIL] {sr.detail}")
        failures.append(("summary", "consistency", sr.detail))

    # MANIFEST hash cross-check.
    print()
    print("MANIFEST.json ↔ git blob sha256 cross-check...")
    if MANIFEST_PATH.exists():
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
        hash_results = verify_manifest_hashes(manifest)
        for fname, ok, detail in hash_results:
            tag = "ok" if ok else "FAIL"
            print(f"  [{tag}] {fname}: {detail[:24]}...")
            if not ok:
                failures.append(("MANIFEST", fname, detail))
    else:
        print("  [WARN] MANIFEST.json not found; skipping hash cross-check")

    # run-metadata cross-check.
    print()
    print("run-metadata.md ↔ run-metadata.json cross-check...")
    r = verify_run_metadata_crosscheck()
    if r:
        print(f"  [ok] {r.detail}")
    else:
        print(f"  [FAIL] {r.detail}")
        failures.append(("run-metadata", "crosscheck", r.detail))

    print()
    if failures:
        print(f"=== {len(failures)} FAILURES ===")
        for inv, eid, detail in failures:
            print(f"  {eid} ({inv}):")
            print(f"    {detail}")
        return 1
    print("All verifications passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
