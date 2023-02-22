use std::{env, net::SocketAddr, time::Duration};

use futures::{stream::FuturesUnordered, StreamExt};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tracing::{error, info, warn};

fn is_prime(val: f64) -> bool {
    let round = val.round();
    if round != val {
        return false;
    }
    let val = round as i64;

    if val < 2 {
        return false;
    }

    if val == 2 {
        return true;
    }

    for x in 2..=(val as f64).sqrt().ceil() as i64 {
        if val % x == 0 {
            return false;
        }
    }

    true
}

#[derive(Deserialize)]
struct Input {
    method: String,
    number: f64,
}

async fn handle_stream(mut stream: TcpStream, addr: SocketAddr) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.split();
    let mut split_reader = BufReader::new(reader).split(b'\n');

    while let Some(segment) = split_reader.next_segment().await? {
        let Input { method, number } = match serde_json::from_slice(&segment) {
            Ok(v) => v,
            Err(e) => {
                warn!("serde_json: {e}");
                writer.write_all(b"gibberish").await?;
                break;
            }
        };

        if method != "isPrime" {
            warn!("invalid method \"{method}\"");
            writer.write_all(b"gibberish\n").await?;
            break;
        }

        let output = serde_json::json!({ "method": "isPrime", "prime": is_prime(number) });
        let mut output_str = serde_json::to_string(&output)?;
        output_str.push('\n');

        writer.write_all(output_str.as_bytes()).await?;
    }

    util::log_and_exit!(addr);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let mut args = env::args();
    args.next().expect("no binary name");

    let addr: SocketAddr = args.next().expect("no addr").parse()?;

    let server = TcpListener::bind(addr).await?;
    let mut futures = FuturesUnordered::new();

    loop {
        tokio::select! {
            res = server.accept() => {
                let (stream, addr) = res?;
                info!("accepted connection from {addr}");
                futures.push(handle_stream(stream, addr));
            }
            opt_res = futures.next() => {
                if let Some(Err(e)) = opt_res {
                    error!("{e}");
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[allow(unreachable_code)]
    Ok(())
}
