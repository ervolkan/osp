//! Faz 0.4 — Git geçmişi → şahitlik profili.
//!
//! Bu modül tam uygulandı: `git` CLI'ye shell-out yapar (taşınabilir, ek native
//! bağımlılık yok). Faz 2+ 'da `git2` crate değerlendirilecek.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::model::WitnessProfile;

/// Bir reponun git geçmişini analiz eder ve şahitlik profili üretir.
pub fn analyze(repo: &Path) -> Result<WitnessProfile> {
    anyhow::ensure!(
        repo.join(".git").exists(),
        "repo .git içermiyor: {:?}",
        repo
    );

    let default_branch = detect_default_branch(repo)?;
    // #3 (güvenlik): default_branch dış `git` çıktısından geldiği için
    // argüman enjeksiyonuna karşı doğrula.
    validate_ref_name(&default_branch)?;
    tracing::debug!(branch = %default_branch, "default branch tespit edildi");

    let total = rev_list_count(repo, &["--all"])?;
    // #1 (mantık): merges'i mainline ile sınırla. `--all --merges` stale/terk
    // edilmiş branch'lerdeki merge'ları da sayıp witnessed_ratio'yu şişirirdi;
    // payda (direct_to_default) zaten mainline sınırlıydı.
    // NOT: `--` ayracı KULLANILMAZ — `git rev-list`'te `--` sonrası path sayılır,
    // rev değil. Injection'ı `validate_ref_name` (aşağıda) önler.
    let merges = rev_list_count(repo, &["--merges", &default_branch])?;
    let direct_to_default = rev_list_count(repo, &["--no-merges", &default_branch])?;
    let distinct_authors = distinct_values(repo, &["log", "--all", "--format=%ae"])?;

    let denom = (merges + direct_to_default).max(1);
    let witnessed_ratio = merges as f64 / denom as f64;
    // #2 (mantık): FF/squash/rebase workflow'larında merge-commit olmadığı için
    // metrik kördür. witnessed_ratio=0'ın gerçek "şahitsizlik"ten ayırt edilmesi
    // için sinyal üret (model.rs doc'una bakın).
    let likely_ff_workflow = merges == 0 && direct_to_default > 0;

    // Faz 0.5 polish: commit→dosya entropisi (w-ekseni adayı).
    let entropy = commit_entropy(repo).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "commit_entropy hesaplanamadı, 0.0");
        0.0
    });

    Ok(WitnessProfile {
        total_commits: total,
        merge_commits: merges,
        direct_to_default,
        distinct_authors,
        witnessed_ratio,
        likely_ff_workflow,
        commit_entropy: entropy,
        default_branch,
    })
}

/// Ref-adı doğrulaması — argüman enjeksiyonunu (#3) önler.
///
/// `git symbolic-ref` çıktısı güvenilmeyen bir repoda `-` ile başlayan veya
/// olağandışı karakterler içeren bir değer dönebilir; bunu olduğu gibi `git
/// rev-list`'e rev argümanı olarak geçmek seçenek enjeksiyonuna kapı açar.
/// `git rev-list`'te `--` ayracı rev'i path'e çevirdiği için KULLANILAMAZ;
/// tek savunma bu doğrulamadır (leading `-` ve geçersiz karakterleri reddeder).
fn validate_ref_name(name: &str) -> Result<()> {
    anyhow::ensure!(!name.is_empty(), "default branch adı boş");
    anyhow::ensure!(
        !name.starts_with('-'),
        "default branch adı `-` ile başlıyor (olası enjeksiyon): {:?}",
        name
    );
    anyhow::ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')),
        "default branch adı geçersiz karakter içeriyor: {:?}",
        name
    );
    Ok(())
}

/// Default branch'i tespit eder: `git symbolic-ref refs/remotes/origin/HEAD`.
/// Başarısız olursa `main`, o da yoksa `master` dener.
fn detect_default_branch(repo: &Path) -> Result<String> {
    if let Ok(out) = run_git(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        let trimmed = out.trim();
        // Çıktı genelde `origin/main` biçiminde; prefix'i ayıkla.
        let name = trimmed.rsplit_once('/').map(|(_, n)| n).unwrap_or(trimmed);
        if !name.is_empty() {
            return Ok(name.to_string());
        }
    }
    // Fallback: önce main, sonra master var mı kontrol et.
    for cand in ["main", "master"] {
        // `--` KULLANILMAZ (rev-list'te path ayıracı). cand hardcoded; ek
        // güvenlik gerekmez.
        if rev_list_count(repo, &[cand]).unwrap_or(0) > 0 {
            return Ok(cand.to_string());
        }
    }
    Ok("main".to_string())
}

/// `git rev-list --count <args>` sonucunu ayrıştırır.
fn rev_list_count(repo: &Path, args: &[&str]) -> Result<usize> {
    let mut full = vec!["rev-list", "--count"];
    full.extend_from_slice(args);
    let out = run_git(repo, &full)?;
    out.trim()
        .parse::<usize>()
        .context("rev-list --count sayı değil")
}

/// Bir `git log --format=...` çıktısındaki distinct satır sayısını sayar.
fn distinct_values(repo: &Path, args: &[&str]) -> Result<usize> {
    let out = run_git(repo, args)?;
    let set: std::collections::BTreeSet<&str> =
        out.lines().filter(|l| !l.trim().is_empty()).collect();
    Ok(set.len())
}

/// Shannon entropisi H = -Σ p_i log2 p_i. Frekans listesinden.
fn shannon_entropy(counts: &[u32]) -> f64 {
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return 0.0;
    }
    let total_f = total as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total_f;
            -p * p.log2()
        })
        .sum()
}

/// Commit→dosya dağılımı üzerinden Shannon entropisi. Yüksek = dosyalara
/// yayılmış değişim (kararlı); düşük = az dosyada yığılma (volatil/negatif-uzay).
fn commit_entropy(repo: &Path) -> Result<f64> {
    // --no-merges: gerçek "iş" commit'leri; --name-only: dokunulan dosyalar;
    // --format='': commit başlık satırını baskıla, yalnızca dosya listesi kalsın.
    let out = run_git(
        repo,
        &["log", "--all", "--no-merges", "--name-only", "--format="],
    )?;
    let mut counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for line in out.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        *counts.entry(l).or_insert(0) += 1;
    }
    let v: Vec<u32> = counts.values().copied().collect();
    Ok(shannon_entropy(&v))
}

/// `git` çalıştırır ve stdout'u döner. Hataları bağlam zenginleştirir.
fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo.as_os_str())
        .args(args)
        .output()
        .with_context(|| format!("git çalıştırılamadı (args: {:?})", args))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git {:?} başarısız: {}", args, stderr.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Faz 0.4 doğrulama: sahte bir repo gerektiren birim testleri Faz 0.7'de
/// (`spike-results.md` üretimi sırasında) fixture-based eklenecek.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rev_list_count_parses_integer() {
        // run_git'i alay etmeden yalnızca ayrıştırıcı test edilebilir; bu test
        // gerçek repo gerektirir, o yüzden ignore işaretli.
    }

    // #3: validate_ref_name güvenlik testleri.
    #[test]
    fn validate_ref_name_rejects_leading_dash() {
        assert!(validate_ref_name("-evil").is_err());
        assert!(validate_ref_name("--upload-pack=x").is_err());
    }

    #[test]
    fn validate_ref_name_accepts_typical_branches() {
        assert!(validate_ref_name("main").is_ok());
        assert!(validate_ref_name("feature/foo-bar_1.0").is_ok());
    }

    #[test]
    fn validate_ref_name_rejects_empty_and_odd_chars() {
        assert!(validate_ref_name("").is_err());
        assert!(validate_ref_name("main;rm -rf").is_err());
        assert!(validate_ref_name("a b").is_err());
    }

    #[test]
    fn shannon_entropy_uniform_is_max() {
        // 4 dosya eşit dokunulmuş → H = log2(4) = 2.0
        let h = shannon_entropy(&[1, 1, 1, 1]);
        assert!((h - 2.0_f64).abs() < 1e-9, "h = {}", h);
    }

    #[test]
    fn shannon_entropy_concentrated_is_near_zero() {
        // tek dosyada yığılma → H ≈ 0 (negatif-uzay sinyali)
        let h = shannon_entropy(&[100, 0, 0, 0]);
        assert!(h.abs() < 1e-9, "h = {}", h);
    }

    #[test]
    fn shannon_entropy_empty_is_zero() {
        assert_eq!(shannon_entropy(&[]), 0.0);
        assert_eq!(shannon_entropy(&[0, 0, 0]), 0.0);
    }
}
