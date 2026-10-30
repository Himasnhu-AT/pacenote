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
    engine::app(pacenote::info()).add_plugins(pacenote::Pacenote).run();
}
