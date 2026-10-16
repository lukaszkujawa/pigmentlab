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

    let mut failures = Vec::new();
    let mut checkpoints = 0;
    for path in &paths {
        let scenario = match Scenario::load(path) {
            Ok(scenario) => scenario,
            Err(error) => {
                failures.push(error);
                continue;
            }
        };
        let report = run(&scenario, &baseline_dir(path), update_baselines);
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        checkpoints += report.checkpoints.len();
        let name = path.file_stem().unwrap().to_string_lossy();
        failures.extend(report.failures().map(|failed| {
            let comparison = failed.comparison.expect("failures carry a comparison");
            let reason = if comparison.missing_baseline {
                "missing baseline".to_string()
            } else {
                format!("{:.2}% of pixels differ, mean difference {:.2}", comparison.differing_fraction * 100.0, comparison.mean_difference)
            };
            format!("{name}/{}: {reason}", failed.name)
        }));
    }
    eprintln!("{} scenarios, {checkpoints} checkpoints, {} failed", paths.len(), failures.len());
    for failure in &failures {
        eprintln!("  FAILED {failure}");
    }
    if !failures.is_empty() {
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
