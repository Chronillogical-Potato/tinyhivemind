//! Offline proof of supplied agents sharing one runtime across dynamic hives.
mod proof;

fn main() -> anyhow::Result<()> {
    proof::runtime()?.block_on(async { tokio::spawn(proof::run()).await })??;
    Ok(())
}
