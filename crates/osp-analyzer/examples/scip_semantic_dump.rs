//! SCIP loader sonucu SemanticIndex'i dump eder — class/method/field/access debug.
use osp_analyzer::scip::load_scip_index;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: scip_semantic_dump <file.scip>");
    let idx = load_scip_index(std::path::Path::new(path))?;

    println!("=== SemanticIndex: {} ===", path);
    println!(
        "classes: {}, files_indexed: {}",
        idx.classes.len(),
        idx.files_indexed
    );
    println!(
        "classes_by_file keys: {:?}",
        idx.classes_by_file.keys().collect::<Vec<_>>()
    );
    println!();

    for class in &idx.classes {
        println!("── Class: {} ──", class.name);
        println!("  methods ({}): {:?}", class.methods.len(), class.methods);
        println!("  fields ({}): {:?}", class.fields.len(), class.fields);
        println!("  field_access ({}):", class.field_access.len());
        for fa in &class.field_access {
            println!("    {} → {}", fa.method, fa.field);
        }
        // LCOM4 hesapla
        use osp_analyzer::scip::lcom4::compute_lcom4;
        let result = compute_lcom4(class);
        println!(
            "  LCOM4={} → cohesion={:.3} (methods={}, fields={}, accesses={})",
            result.lcom4,
            result.cohesion(),
            result.method_count,
            result.field_count,
            result.access_count
        );
        println!();
    }

    Ok(())
}
