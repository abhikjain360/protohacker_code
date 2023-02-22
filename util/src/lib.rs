use std::{env, future::Future, net::SocketAddr};

use anyhow::anyhow;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

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

pub async fn accept_loop<F, Fut, State>(f: F, addr: SocketAddr, state: State) -> anyhow::Result<()>
where
    Fut: Future<Output = anyhow::Result<()>> + Send,
    F: FnOnce(TcpStream, State) -> Fut + Copy + Sync + Send + 'static,
    State: Send + Clone + 'static,
{
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, addr) = listener.accept().await?;
        info!("accepted connection from {addr}");

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = f(stream, state).await {
                error!("{e}");
            }
            info!("closing connection with {addr}");
        });
    }

    #[allow(unreachable_code)]
    Ok(())
}

pub async fn accept_loop_with_env<F, Fut, State>(f: F, state: State) -> anyhow::Result<()>
where
    Fut: Future<Output = anyhow::Result<()>> + Send,
    F: FnOnce(TcpStream, State) -> Fut + Copy + Sync + Send + 'static,
    State: Send + Clone + 'static,
{
    init_tracing();
    accept_loop(f, addr_from_args()?, state).await
}
