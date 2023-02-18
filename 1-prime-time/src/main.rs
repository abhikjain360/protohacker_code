use std::{env, net::SocketAddr};

use futures::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{info, warn};

struct ReadHandler {
    addr: SocketAddr,
    stream: TcpStream,
    closed: bool,
}

impl ReadHandler {
    async fn try_read(self) -> anyhow::Result<WriteHandler> {
        let ReadHandler {
            mut stream, addr, ..
        } = self;

        let mut read_bytes = 0;
        let mut buf = vec![0; 4096];
        let mut closed;

        let input = loop {
            let n = stream.read(&mut buf[read_bytes..]).await?;
            closed = n == 0;
            read_bytes += n;

            match serde_json::from_slice::<WebInput>(&buf[..read_bytes]) {
                Ok(input) if !input.is_valid() => {
                    warn!("invalid input method \"{}\"", input.method);
                    break None;
                }
                Ok(input) => {
                    let rounded = input.number.round();
                    if rounded != input.number {
                        break Some(-1);
                    } else {
                        break Some(rounded as i64);
                    }
                }

                Err(_) if closed => {
                    break None;
                }
                Err(e) if buf[read_bytes - 1] == b'\n' => {
                    warn!("serde_json: {e}");
                    break None;
                }
                _ => {}
            }

            if read_bytes == buf.len() {
                buf.resize(read_bytes + 1024, 0);
            }
        };

        Ok(WriteHandler {
            addr,
            stream,
            input,
            closed,
        })
    }
}

struct WriteHandler {
    addr: SocketAddr,
    stream: TcpStream,
    input: Option<i64>,
    closed: bool,
}

impl WriteHandler {
    async fn try_write(self) -> anyhow::Result<ReadHandler> {
        let WriteHandler {
            mut stream,
            input,
            addr,
            closed,
        } = self;

        if let Some(input) = input {
            let output = &WebOutput::new(check_prime(input));
            let mut output_str = serde_json::to_string(output)?;
            output_str.push('\n');

            stream.write_all(output_str.as_bytes()).await?;
        } else {
            stream.write_all(b"gibberish\n").await?;
        };

        Ok(ReadHandler {
            addr,
            stream,
            closed,
        })
    }
}

#[derive(Deserialize)]
struct WebInput {
    method: String,
    number: f64,
}

impl WebInput {
    fn is_valid(&self) -> bool {
        self.method == "isPrime"
    }
}

#[derive(Serialize)]
struct WebOutput {
    method: String,
    prime: bool,
}

impl WebOutput {
    fn new(is_prime: bool) -> Self {
        Self {
            method: "isPrime".to_string(),
            prime: is_prime,
        }
    }
}

fn check_prime(val: i64) -> bool {
    if val < 2 {
        return false;
    };

    for x in 2..(val as f64).sqrt().ceil() as i64 {
        if val % x == 0 {
            return false;
        }
    }

    return true;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let mut args = env::args();
    args.next().expect("no binary name");

    let addr: SocketAddr = args.next().expect("no bind addr provided").parse()?;

    let listener = TcpListener::bind(addr).await?;

    let read_futures = &mut FuturesUnordered::new();
    let write_futures = &mut FuturesUnordered::new();

    loop {
        let new_conn_fut = listener.accept();
        let conn_read_fut = read_futures.next();
        let conn_write_fut = write_futures.next();
        tokio::select! {
            res = new_conn_fut => {
                let (stream, addr) = res?;
                let handler = ReadHandler {
                    addr,
                    stream,
                    closed: false,
                };
                info!("accepted connection from {addr:?}");
                read_futures.push(handler.try_read());
            },
            opt = conn_read_fut => {
                let handler = match opt {
                    None => continue,
                    Some(res) => res?
                };


                if handler.closed {
                    info!("connection closed with {}", handler.addr);
                } else {
                    write_futures.push(handler.try_write());
                }
            },
            opt = conn_write_fut => {
                let handler = match opt {
                    None => continue,
                    Some(res) => res?
                };

                if handler.closed {
                    info!("connection closed with {}", handler.addr);
                } else {
                    read_futures.push(handler.try_read());
                }
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}
