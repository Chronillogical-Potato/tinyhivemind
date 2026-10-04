//! Host construction, actual provider captures, and continuing sessions.
mod dynamic;
mod fixture;
#[cfg(test)]
mod test;
mod topology;
mod types;

pub fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?)
}

pub async fn run() -> anyhow::Result<()> {
    for (agents, hives) in [(1, 1), (3, 1), (3, 2), (1, 3)] {
        topology::run(agents, hives).await?;
        println!("supplied agents: {agents}; hives: {hives}; continuing sessions verified");
    }
    dynamic::run().await?;
    Ok(())
}
