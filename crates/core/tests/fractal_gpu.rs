use pigmentlab_core::scenario::{Scenario, run};
use std::path::Path;

#[test]
fn a_pollock_style_pour_has_the_fractal_dimension_of_pollocks_drip_paintings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scenario = Scenario::load(&root.join("scenarios/drip_pollock.json")).expect("the Pollock scenario loads");
    let report = run(&scenario, &root.join("baselines/drip_pollock"), false);
    for checkpoint in &report.checkpoints {
        let dimension = checkpoint.fractal_dimension.expect("the pour paints the canvas");
        println!("{}: box-counting dimension {dimension:.3}", checkpoint.name);
        assert!((1.5..1.8).contains(&dimension), "{} measured {dimension:.3}; Taylor et al. measure 1.5-1.8 in Pollock's drip paintings", checkpoint.name);
    }
}
