fn main() {
    // Release builds carry the telemetry endpoint and write key (docs/TELEMETRY.md) from build
    // time; the same variables at run time win. The write key only allows ingest.
    for (k, v) in [("PACENOTE_TRACE_ENDPOINT", option_env!("PACENOTE_TRACE_ENDPOINT")), ("PACENOTE_TRACE_KEY", option_env!("PACENOTE_TRACE_KEY"))] {
        if let Some(v) = v
            && std::env::var_os(k).is_none()
        {
            // SAFETY: first thing in main, before any other thread exists.
            unsafe { std::env::set_var(k, v) };
        }
    }
    // `--bake-world <file>`: write the generated open world as a level and exit.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--bake-world") {
        let Some(path) = args.get(i + 1) else {
            eprintln!("usage: pacenote --bake-world levels/open.level.ron");
            std::process::exit(2);
        };
        match pacenote::open::level::bake(std::path::Path::new(path)) {
            Ok(report) => println!("{report}"),
            Err(e) => {
                eprintln!("bake failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    engine::app(pacenote::info()).add_plugins(pacenote::Pacenote).run();
}
