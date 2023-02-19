use std::{collections::BTreeMap, env, net::SocketAddr};

use futures::{stream::FuturesUnordered, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{error, info};

macro_rules! allow_eof {
    ($expr:expr) => {
        match $expr {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
    };
}

async fn handle_stream(mut stream: TcpStream, addr: SocketAddr) -> anyhow::Result<()> {
    let mut tree = BTreeMap::new();
    loop {
        match allow_eof!(stream.read_u8().await) {
            b'I' => handle_insert(&mut stream, &mut tree).await?,
            b'Q' => handle_query(&mut stream, &tree).await?,
            b => {
                error!("invalid operation byte: {b}");
                break;
            }
        }
    }

    info!("closing connection with {addr}");
    Ok(())
}

async fn handle_insert(
    stream: &mut TcpStream,
    tree: &mut BTreeMap<i32, i64>,
) -> anyhow::Result<()> {
    let timestamp = stream.read_i32().await?;
    let price = stream.read_i32().await? as i64;
    tree.insert(timestamp, price);
    Ok(())
}

async fn handle_query(stream: &mut TcpStream, tree: &BTreeMap<i32, i64>) -> anyhow::Result<()> {
    let start = stream.read_i32().await?;
    let end = stream.read_i32().await?;

    if start > end {
        stream.write_i32(0).await?;
        return Ok(());
    }

    let (len, sum) = tree
        .range(start..=end)
        .fold((0, 0), |(len, sum), (_, &price)| (len + 1, sum + price));

    let avg = if len == 0 { 0 } else { sum / len } as i32;

    stream.write_i32(avg).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let mut args = env::args();
    args.next().expect("no binary name");

    let addr: SocketAddr = args.next().expect("no addr").parse()?;

    let server = TcpListener::bind(addr).await?;

    let mut connections = FuturesUnordered::new();

    loop {
        tokio::select! {
            res = server.accept() => {
                let (stream, addr) = res?;
                info!("accepted connection from {addr}");
                connections.push(handle_stream(stream, addr));
            }
            opt_res = connections.next() => {
                if let Some(Err(e)) = opt_res {
                    error!("{e}");
                }
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}
