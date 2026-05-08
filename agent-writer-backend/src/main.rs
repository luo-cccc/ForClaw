#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::panic;

fn main() {
    // Init tracing subscriber with rolling daily file appender
    let log_dir = dirs_next().unwrap_or_else(|| std::path::PathBuf::from("."));
    let file_appender = tracing_appender::rolling::daily(&log_dir, "agent-writer.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter("agent_writer=info,agent_harness_core=info")
        .with_writer(non_blocking)
        .with_target(false)
        .init();

    // Panic hook: log to tracing + show native dialog
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let msg = format!(
            "PANIC: {}\nBacktrace:\n{}",
            info,
            std::backtrace::Backtrace::capture()
        );
        tracing::error!("{}", msg);
        // Try native message box
        let _ = msgbox::create("Agent-Writer Error", &msg, msgbox::IconType::Error);
        default_hook(info);
    }));

    tracing::info!("Agent-Writer starting");
    agent_writer_lib::run()
}

fn dirs_next() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|p| {
            std::path::PathBuf::from(p)
                .join("agent-writer")
                .join("logs")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir().map(|p| p.join(".config").join("agent-writer").join("logs"))
    }
}
