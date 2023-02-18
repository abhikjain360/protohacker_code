use std::{env, io, net::SocketAddr};

use futures::{stream::FuturesUnordered, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::info;

struct StreamHandler {
    addr: SocketAddr,
    stream: TcpStream,
    closed: bool,
    buf: Vec<u8>,
    write_to: usize,
}

impl StreamHandler {
    async fn try_read(mut self) -> io::Result<Self> {
        let StreamHandler {
            stream,
            buf,
            write_to,
            closed,
            ..
        } = &mut self;

        let bytes_read = stream.read(&mut buf[*write_to..]).await?;
        *write_to += bytes_read;
        *closed = bytes_read == 0;

        Ok(self)
    }

    async fn try_write(mut self) -> io::Result<Self> {
        let StreamHandler {
            stream,
            buf,
            write_to,
            ..
        } = &mut self;

        stream.write_all(&buf[..*write_to]).await?;
        *write_to = 0;

        Ok(self)
    }
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
                let handler = StreamHandler {
                    addr,
                    stream,
                    closed: false,
                    buf: vec![0; 4096],
                    write_to: 0,
                };
                info!("accepted connection from {addr:?}");
                read_futures.push(handler.try_read());
            },
            opt = conn_read_fut => {
                let handler = match opt {
                    None => continue,
                    Some(res) => res?
                };

                write_futures.push(handler.try_write());
            },
            opt = conn_write_fut => {
                let handler = match opt {
                    None => continue,
                    Some(res) => res?
                };

                if !handler.closed {
                    read_futures.push(handler.try_read());
                } else {
                    info!("connection closed with {}", handler.addr);
                }
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}
