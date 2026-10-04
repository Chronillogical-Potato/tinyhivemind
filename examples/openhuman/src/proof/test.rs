// OpenHuman intentionally permits one runtime per process.
static RUNTIME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn four_topologies_preserve_supplied_configuration_and_permanent_tools() {
    let _serial = RUNTIME_LOCK.lock().unwrap();
    super::runtime()
        .unwrap()
        .block_on(async {
            tokio::spawn(async {
                for (agents, hives) in [(1, 1), (3, 1), (3, 2), (1, 3)] {
                    super::topology::run(agents, hives).await?;
                }
                anyhow::Ok(())
            })
            .await
        })
        .expect("proof task")
        .expect("topology captures");
}

#[test]
fn native_management_creates_configured_agents_and_returns_failures() {
    let _serial = RUNTIME_LOCK.lock().unwrap();
    super::runtime()
        .unwrap()
        .block_on(async { tokio::spawn(super::dynamic::run()).await })
        .expect("proof task")
        .expect("dynamic captures");
}
