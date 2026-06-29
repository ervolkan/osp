//! SCIP index diagnostic — .scip dosyasının içeriğini dump eder (symbols + occurrences).
//! Kullanım: cargo run --example scip_dump -- <file.scip>

use protobuf::Message;
use scip::types::Index;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: scip_dump <file.scip>");
    let bytes = std::fs::read(path)?;
    let index = Index::parse_from_bytes(&bytes)?;

    println!("=== SCIP Index Dump: {} ===", path);
    println!("Documents: {}", index.documents.len());
    println!();

    for doc in &index.documents {
        println!("── Document: {:?} ──", doc.relative_path);
        println!(
            "  Symbols: {}, Occurrences: {}",
            doc.symbols.len(),
            doc.occurrences.len()
        );

        // --- Symbols with kind inference ---
        println!("\n  Symbols (first 15):");
        for (i, sym) in doc.symbols.iter().enumerate().take(15) {
            let kind_val = sym.kind.value();
            println!("    [{}] kind={} symbol={:?}", i, kind_val, sym.symbol);
        }

        // --- Occurrences with full detail ---
        println!("\n  Occurrences (all):");
        for (i, occ) in doc.occurrences.iter().enumerate() {
            let roles = occ.symbol_roles;
            let is_def = roles & 1 != 0;
            let tag = if is_def { "DEF" } else { "ref" };
            let range = &occ.range;
            let last_seg = occ.symbol.rsplit(' ').next().unwrap_or(&occ.symbol);
            println!(
                "    [{}] {} range={:?} sym_suffix={:?}",
                i, tag, range, last_seg
            );
        }
        println!();
    }

    Ok(())
}
