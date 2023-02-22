use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = env::args().nth(1).expect("no addr provided").parse()?;

    #[allow(unreachable_code)]
    Ok(())
}
