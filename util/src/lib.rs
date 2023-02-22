use std::{env, net::SocketAddr};

use anyhow::anyhow;

#[macro_export]
macro_rules! log_and_exit {
    ($addr:ident) => {
        tracing::info!("closing connection with {}", $addr);
        return Ok(());
    };
}

#[macro_export]
macro_rules! write_and_exit {
    ($writer:ident, $msg:ident, $addr:ident) => {
        tokio::io::AsyncWriteExt::write_all(&mut $writer, $msg).await?;
        log_and_exit!($addr);
    };
}

pub fn addr_from_args() -> anyhow::Result<SocketAddr> {
    env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("no addr provided in arguments"))?
        .parse()
        .map_err(Into::into)
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init()
}
