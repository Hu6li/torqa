//! Inspect Android's provisional estimate without building or rendering a world.
//! `cargo run -p orqwa-app --example memory_estimate -- course.oqc [upload_radius_m] [cores] [max_additional_gb]`

use std::path::PathBuf;

use orqwa_domain::files::UsedFiles;
use orqwa_domain::units::Meters;
use orqwa_storage::course;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = PathBuf::from(args.first().expect("course.oqc [upload_radius_m] [cores]"));
    let radius = args
        .get(1)
        .map_or(3925.0, |s| s.parse().expect("radius in metres"));
    let cores = args
        .get(2)
        .map_or(8, |s| s.parse().expect("number of cores"));
    let maximum = args
        .get(3)
        .map(|s| s.parse::<f64>().expect("maximum additional GB"));
    let dir = std::env::temp_dir().join(format!("orqwa-memory-estimate-{}", std::process::id()));
    let unpacked = course::unpack(&path, &dir).expect("course inputs");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let imported = runtime
        .block_on(orqwa_app::import_gpx(
            unpacked.gpx,
            &unpacked.manifest.name,
            (&dir, true),
            (false, &unpacked.manifest.map_detail),
            &UsedFiles::default(),
            &mut |_, _, _| {},
        ))
        .expect("offline course inputs");
    let estimate =
        orqwa_app::memory::estimate(&imported.route, &imported.map, Meters(radius), cores);
    println!(
        "{}; radius {radius} m; {cores} cores",
        unpacked.manifest.name
    );
    println!("preparation: {}", gb(estimate.preparation));
    println!("batch: {}", gb(estimate.batch));
    println!("scene/whole parts: {}", gb(estimate.ride));
    println!("building: {}", gb(estimate.build));
    println!(
        "map: {} roads; {} buildings; {} areas",
        imported.map.roads.len(),
        imported.map.buildings.len(),
        imported.map.areas.len()
    );
    std::fs::remove_dir_all(dir).expect("remove temporary inputs");
    if let Some(maximum) = maximum {
        #[allow(clippy::cast_precision_loss)] // a human-readable diagnostic threshold
        let additional = estimate.build as f64 / 1_000_000_000.0;
        assert!(
            additional <= maximum,
            "estimated {additional:.3} GB exceeds {maximum:.3} GB"
        );
    }
}

#[allow(clippy::cast_precision_loss)] // human-readable diagnostic, bytes remain integers in policy
fn gb(bytes: u64) -> String {
    format!("{:.3} GB", bytes as f64 / 1_000_000_000.0)
}
