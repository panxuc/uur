//! Keep the desktop session awake only while the managed UU session is alive.

use anyhow::Result;
use ashpd::desktop::inhibit::{InhibitFlags, InhibitProxy};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub fn serve() -> Result<()> {
    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stopping.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stopping.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGHUP, stopping.clone())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let proxy = InhibitProxy::new().await?;
        let request = proxy
            .inhibit(
                None,
                InhibitFlags::Suspend | InhibitFlags::Idle,
                "An active UU Remote session is running",
            )
            .await?;
        request.response()?;
        println!("desktop suspend and idle inhibition active");
        while !stopping.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    })
}
