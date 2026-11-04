//! Developer diagnostic: resolves a magnet with a plain librqbit session and
//! prints what the engine sees. Not shipped in the app binary.

use std::time::Duration;

use anyhow::Context;
use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "librqbit=debug,magnet_probe=debug".into()),
        )
        .init();

    let magnet = std::env::args().nth(1).context("magnet argument missing")?;
    let dir = std::env::temp_dir().join("nori-magnet-probe");
    std::fs::create_dir_all(&dir)?;

    let mut opts = SessionOptions::default();
    opts.ipv4_only = true;
    opts.listen = Some(librqbit::ListenerOptions {
        listen_addr: (std::net::Ipv4Addr::UNSPECIFIED, 0).into(),
        ..Default::default()
    });
    opts.disable_local_service_discovery = true;

    let session = Session::new_with_opts(dir, opts).await?;
    println!("listen addr: {:?}", session.listen_addr());

    let started = std::time::Instant::now();
    let response = tokio::time::timeout(
        Duration::from_secs(90),
        session.add_torrent(
            AddTorrent::from_url(magnet),
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        ),
    )
    .await??;
    let Some(handle) = response.into_handle() else {
        println!("no handle");
        return Ok(());
    };
    println!(
        "metadata resolved in {:?}: {} files",
        started.elapsed(),
        handle.with_metadata(|m| m.file_infos.len()).unwrap_or(0)
    );
    for i in 0..10 {
        let stats = handle.stats();
        println!(
            "t={}s state={:?} peers={:?} progress={} down={}",
            i * 2,
            stats.state,
            stats.live.as_ref().map(|l| l.snapshot.peer_stats.live),
            stats.progress_bytes,
            stats.live.as_ref().map(|l| l.download_speed.mbps).unwrap_or(0.0),
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}
