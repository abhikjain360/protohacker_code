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

fn is_wallet_addr(segment: &[u8]) -> bool {
    !segment.is_empty()
        && segment[0] == b'7'
        && segment.len() >= 26
        && segment.len() <= 35
        && segment.iter().all(u8::is_ascii_alphanumeric)
}

fn replace_wallet(msg: Vec<u8>) -> Vec<u8> {
    if msg.is_empty() {
        return msg;
    }

    let Some(start) = msg
        .iter()
        .position(|b| !b.is_ascii_whitespace())
    else { return msg };
    let end = match msg[start..].iter().position(|b| b.is_ascii_whitespace()) {
        Some(idx) => idx + start,
        None => msg.len(),
    };
    let segment = &msg[start..end];

    if is_wallet_addr(segment) {
        let mut new_msg = msg[..start].to_vec();
        new_msg.extend_from_slice(TONY_WALLET);
        new_msg.extend_from_slice(&msg[end..]);
        return new_msg;
    }

    let Some(position) = msg
            .iter()
            .rev()
            .position(|b| !b.is_ascii_whitespace()) else { return msg };
    let end = msg.len() - position;
    let start = match msg[..end]
        .iter()
        .rev()
        .position(|b| b.is_ascii_whitespace())
    {
        Some(idx) => msg.len() - idx,
        None => 0,
    };
    let segment = &msg[start..end];

    if is_wallet_addr(segment) {
        let mut new_msg = msg[..start].to_vec();
        new_msg.extend_from_slice(TONY_WALLET);
        new_msg.extend_from_slice(&msg[end..]);
        return new_msg;
    }

    msg
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
                    Some(client_msg) => replace_wallet(client_msg),
                    None => break,
                };
                msg.push(b'\n');
                upstream_writer.write_all(&msg).await?;
            }
            upstream_msg_res = upstream_lines.next_segment() => {
                let mut msg = match upstream_msg_res? {
                    Some(upstream_msg) => replace_wallet(upstream_msg),
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
