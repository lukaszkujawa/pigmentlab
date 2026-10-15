use simpaint_core::scenario::{Scenario, run};
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let update_baselines = args.iter().any(|a| a == "--update-baselines");
    let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: scenario [--update-baselines] <scenario.json>...");
        std::process::exit(2);
    }

    let mut all_passed = true;
    for path in &paths {
        let scenario = Scenario::load(path).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        });
        let report = run(&scenario, &baseline_dir(path), update_baselines);
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        all_passed &= report.passed();
    }
    if !all_passed {
        std::process::exit(1);
    }
}

fn baseline_dir(scenario_path: &Path) -> PathBuf {
    let name = scenario_path.file_stem().unwrap().to_string_lossy();
    scenario_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."))
        .join("baselines")
        .join(name.as_ref())
}
