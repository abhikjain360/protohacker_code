use std::{env, net::SocketAddr};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tracing::{error, info};

macro_rules! log_and_exit {
    ($addr:ident) => {
        info!("closing connection with {}", $addr);
        return Ok(());
    };
}

const UPSTREAM_ADDR: &str = "chat.protohackers.com:16963";
const TONY_WALLET: &[u8] = b"7YWHMfk9JZe0LM0g1ZauHuiSxhI";

fn is_boguscoin(segment: &[u8]) -> bool {
    segment.len() >= 26 && segment.len() <= 35 && segment[0] == b'7'
}

fn replace_wallet(message: &[u8]) -> Vec<u8> {
    let mut result = message
        .split(u8::is_ascii_whitespace)
        .map(|segment| {
            if is_boguscoin(segment) {
                return TONY_WALLET;
            }
            segment
        })
        .fold(vec![], |mut v, segment| {
            v.extend_from_slice(segment);
            v.push(b' ');
            v
        });
    result.pop();
    result
}

async fn handle_stream(mut stream: TcpStream, addr: SocketAddr) -> anyhow::Result<()> {
    let mut upstream = TcpStream::connect(UPSTREAM_ADDR).await?;
    let (upstream_reader, mut upstream_writer) = upstream.split();
    let mut upstream_lines = BufReader::new(upstream_reader).split(b'\n');

    let (client_reader, mut client_writer) = stream.split();
    let mut client_lines = BufReader::new(client_reader).split(b'\n');

    loop {
        tokio::select! {
            client_msg_res = client_lines.next_segment() => {
                let mut msg = match client_msg_res? {
                    Some(client_msg) => replace_wallet(&client_msg),
                    None => break,
                };
                msg.push(b'\n');
                upstream_writer.write_all(&msg).await?;
            }
            upstream_msg_res = upstream_lines.next_segment() => {
                let mut msg = match upstream_msg_res? {
                    Some(upstream_msg) => replace_wallet(&upstream_msg),
                    None => break,
                };
                msg.push(b'\n');
                client_writer.write_all(&msg).await?;
            }
        }
    }

    log_and_exit!(addr);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let mut args = env::args();
    args.next().expect("no binary name");

    let addr: SocketAddr = args.next().expect("no addr").parse()?;

    let server = TcpListener::bind(addr).await?;

    loop {
        let (stream, addr) = server.accept().await?;
        info!("accepted connection from {addr}");

        tokio::spawn(async move {
            if let Err(e) = handle_stream(stream, addr).await {
                error!("{e}");
            }
        });
    }

    #[allow(unreachable_code)]
    Ok(())
}
