//! Replay a recorded touch stream through the engine - the "local fake client"
//! of plan.md section 13.
//!
//! Two modes:
//!   cargo run --bin replay -- tests/fixtures/two_finger_scroll.json
//!       print the actions the engine produces (safe, no injection)
//!   cargo run --bin replay -- --inject tests/fixtures/one_finger_move.json
//!       actually drive the cursor, at the timing the stream was recorded at

use std::path::PathBuf;

use anyhow::{Context, Result};
use padremote::gesture::{Config, Recognizer, TouchSample};
use padremote::input::Injector;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    surface: Surface,
    samples: Vec<Sample>,
}

#[derive(Deserialize)]
struct Surface {
    wpx: f64,
    hpx: f64,
}

#[derive(Deserialize)]
struct Sample {
    t_ms: u32,
    pointer_id: u8,
    phase: u8,
    x: f32,
    y: f32,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let inject = args.iter().any(|a| a == "--inject");
    let paths: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        eprintln!("usage: replay [--inject] <fixture.json>...");
        std::process::exit(2);
    }

    let mut injector: Option<Box<dyn Injector>> = if inject {
        #[cfg(target_os = "macos")]
        {
            use padremote::input::{accessibility_trusted, permission_help, PlatformInjector};
            if !accessibility_trusted() {
                eprintln!("{}", permission_help());
                anyhow::bail!("Accessibility permission is not granted");
            }
            Some(Box::new(PlatformInjector::new()?))
        }
        #[cfg(not(target_os = "macos"))]
        {
            anyhow::bail!("--inject has no backend on this platform yet")
        }
    } else {
        None
    };

    for path in paths {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let f: Fixture = serde_json::from_str(&text)
            .with_context(|| format!("cannot parse {}", path.display()))?;

        let name = if f.name.is_empty() {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            f.name.clone()
        };
        println!("\n=== {name} ===");
        if !f.description.is_empty() {
            println!("{}", f.description);
        }

        let mut rec = Recognizer::new(Config::default(), f.surface.wpx, f.surface.hpx);
        let mut last_t = f.samples.first().map(|s| s.t_ms).unwrap_or(0);
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();

        for s in &f.samples {
            if injector.is_some() {
                // Replay at the pace it was recorded, so the result is what the
                // user would actually feel.
                let dt = s.t_ms.saturating_sub(last_t);
                if dt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(dt as u64));
                }
            }
            last_t = s.t_ms;

            let sample = TouchSample {
                t_ms: s.t_ms,
                pointer_id: s.pointer_id,
                phase: s.phase,
                x: s.x,
                y: s.y,
            };
            for action in rec.feed(&[sample]) {
                *counts.entry(action.kind()).or_default() += 1;
                match &mut injector {
                    Some(inj) => inj.apply(action),
                    None => println!("  {action:?}"),
                }
            }
        }
        // A stream that ends mid-gesture must not leave anything held.
        for action in rec.release_all() {
            *counts.entry(action.kind()).or_default() += 1;
            if let Some(inj) = &mut injector {
                inj.apply(action);
            }
        }
        let summary: Vec<String> = counts.iter().map(|(k, n)| format!("{k}x{n}")).collect();
        println!("  -> {}", summary.join(", "));
    }
    Ok(())
}
