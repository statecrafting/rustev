//! Standalone service for exact plans. Ecosystem backends remain host supplied.
#![forbid(unsafe_code)]
use rustev_contract::{Document, serve::ServeConfig};
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("rustev-serve: {error}");
        std::process::exit(3);
    }
}
async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--config" {
        return Err("usage: rustev-serve --config FILE".into());
    }
    let config = ServeConfig::parse(&rustev_serve::transport::read_file(
        &args[1],
        4 * 1024 * 1024,
    )?)
    .map_err(|e| e.to_string())?;
    let service = rustev_serve::sinks::standalone(config).await?;
    let mut tasks = tokio::task::JoinSet::new();
    for listener in service.config.listeners.as_slice() {
        let service = service.clone();
        let listener = listener.clone();
        tasks.spawn(async move { rustev_serve::transport::listen(service, listener).await });
    }
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => (),
        _ = async { #[cfg(unix)] { term.recv().await; } #[cfg(not(unix))] { std::future::pending::<()>().await; } } => (),
        result = tasks.join_next() => return Err(format!("listener stopped: {result:?}")),
    }
    let normal = service.shutdown(false);
    tokio::pin!(normal);
    let (outcome, immediate) = tokio::select! {
        r = &mut normal => (r, false),
        _ = tokio::signal::ctrl_c() => (service.shutdown(true).await, true),
        _ = async { #[cfg(unix)] { term.recv().await; } #[cfg(not(unix))] { std::future::pending::<()>().await; } } => (service.shutdown(true).await, true),
    };
    let (report, exit) = outcome;
    eprintln!(
        "{}",
        String::from_utf8_lossy(&report.record_canonical().map_err(|e| e.to_string())?)
    );
    // Keep transports alive for terminal events after decision and sink drain.
    if !immediate {
        let finish_connections = tokio::time::timeout(
            std::time::Duration::from_millis(service.config.flush_ms.get()),
            async { while tasks.join_next().await.is_some() {} },
        );
        tokio::select! {
            _ = finish_connections => (),
            _ = tokio::signal::ctrl_c() => (),
            _ = async { #[cfg(unix)] { term.recv().await; } #[cfg(not(unix))] { std::future::pending::<()>().await; } } => (),
        }
    }
    tasks.abort_all();
    if exit != 0 {
        std::process::exit(exit as i32);
    }
    Ok(())
}
