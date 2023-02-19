use std::{env, net::SocketAddr};

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

async fn handle_stream(
    mut stream: TcpStream,
    addr: SocketAddr,
    tree: sled::Tree,
) -> anyhow::Result<()> {
    loop {
        match allow_eof!(stream.read_u8().await) {
            b'I' => handle_insert(&mut stream, &tree).await?,
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

async fn handle_insert(stream: &mut TcpStream, tree: &sled::Tree) -> anyhow::Result<()> {
    let timestamp = stream.read_i32().await?;
    let price = stream.read_i32().await? as i64;
    tree.insert(timestamp.to_be_bytes(), &price.to_be_bytes())?;
    Ok(())
}

async fn handle_query(stream: &mut TcpStream, tree: &sled::Tree) -> anyhow::Result<()> {
    let start = stream.read_i32().await?;
    let end = stream.read_i32().await?;

    let (len, sum) = tree
        .range(start.to_be_bytes()..=end.to_be_bytes())
        .try_fold((0, 0), |(len, sum), res| {
            res.map(|(_, v)| {
                (
                    len + 1,
                    sum + i64::from_be_bytes([v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]]),
                )
            })
        })?;

    stream.write_i32((sum / len) as i32).await?;

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
    let db = sled::open("clients").expect("unable to open db");

    loop {
        tokio::select! {
            res = server.accept() => {
                let (stream, addr) = res?;
                info!("accepted connection from {addr}");
                let tree = db.open_tree(addr.to_string().as_bytes())?;
                connections.push(handle_stream(stream, addr, tree));
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
