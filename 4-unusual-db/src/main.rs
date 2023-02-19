use std::{env, net::SocketAddr};

use tokio::net::UdpSocket;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let addr: SocketAddr = env::args().nth(1).expect("no addr").parse()?;

    let socket = UdpSocket::bind(addr).await?;
    let buf = &mut vec![0; 1000];

    let db = sled::open("db")?;

    loop {
        let (bytes_read, addr) = socket.recv_from(buf).await?;
        let mut data = buf[..bytes_read].splitn(2, |b| *b == b'=');

        let key = data.next().expect("there should atleast be an empty slice");

        info!("addr = {addr}, key = {key:?}");

        if key == b"version" {
            socket.send_to(b"version=Abhik's attempt at Protohack Q4: v1.1", addr).await?;
            continue;
        }

        match data.next() {
            None => {
                info!("query");

                let value = db.get(key)?.unwrap_or(b"".into());

                buf[bytes_read] = b'=';
                let start = bytes_read + 1;
                let end = start + value.len();
                buf[start..end].copy_from_slice(&value);

                socket.send_to(&buf[..end], addr).await?;
            }
            Some(value) => {
                info!("insert");
                db.insert(key, value)?;
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}
