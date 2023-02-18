use std::{
    env,
    io::{ErrorKind, Read, Write},
    net::SocketAddr,
    time::Duration,
};

use ahash::RandomState;
use mio::{
    event::Event,
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};
use tracing::{error, info};

const SERVER_TOKEN: Token = Token(0);
const TIMEOUT: Duration = Duration::from_millis(100);

type HashMap<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;

struct StreamHandler {
    stream: TcpStream,
    addr: SocketAddr,
    buf: Vec<u8>,
    to_write: usize,
}

impl StreamHandler {
    fn with_stream(stream: TcpStream, addr: SocketAddr) -> Self {
        Self {
            stream,
            buf: vec![0; 4096],
            to_write: 0,
            addr,
        }
    }

    fn try_rw(&mut self, event: &Event) -> anyhow::Result<bool> {
        let mut close = false;

        if event.is_readable() {
            close = self.try_read()?;
        }

        if event.is_writable() {
            self.try_write()?;
        }

        Ok(close)
    }

    fn try_read(&mut self) -> anyhow::Result<bool> {
        let StreamHandler {
            stream,
            buf,
            to_write,
            ..
        } = self;
        let mut bytes_read = 0;
        let mut close = false;

        // read until we are blocked or an error occurs
        loop {
            match stream.read(&mut buf[*to_write..]) {
                Err(e) => match e.kind() {
                    ErrorKind::Interrupted => continue,
                    ErrorKind::WouldBlock => break,
                    _ => return Err(e.into()),
                },
                Ok(0) => {
                    close = true;
                    break;
                }
                Ok(n) => bytes_read += n,
            }

            if *to_write == buf.len() {
                buf.resize(*to_write + 1024, 0);
            }
        }

        *to_write += bytes_read;
        if *to_write == buf.len() {
            buf.resize(*to_write + 1024, 0);
        }

        Ok(close || bytes_read == 0)
    }

    fn try_write(&mut self) -> anyhow::Result<()> {
        let StreamHandler {
            stream,
            buf,
            to_write,
            ..
        } = self;
        loop {
            match stream.write_all(&buf[..*to_write]) {
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock => break,
                    ErrorKind::Interrupted => continue,
                    _ => return Err(e.into()),
                },
                Ok(_) => break,
            }
        }

        *to_write = 0;

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = env::args();
    args.next().expect("no binary name provided");
    let socke_addr = args.next().expect("no port number provided").parse()?;

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .pretty()
        .init();

    let poll = &mut Poll::new()?;
    let events = &mut Events::with_capacity(1024);

    let stream_interests = Interest::READABLE | Interest::WRITABLE;

    let server = &mut TcpListener::bind(socke_addr)?;
    poll.registry()
        .register(server, SERVER_TOKEN, stream_interests)?;

    let connections = &mut HashMap::with_capacity_and_hasher(1024, RandomState::new());
    let mut token_index = 1;

    loop {
        poll.poll(events, Some(TIMEOUT))?;

        for event in &*events {
            match event.token() {
                SERVER_TOKEN => loop {
                    let (mut stream, addr) = match server.accept() {
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => {
                            error!("{e:?} => {e}");
                            break;
                        }
                        Ok(v) => v,
                    };

                    let token = Token(token_index);
                    token_index += 1;

                    poll.registry()
                        .register(&mut stream, token, stream_interests)?;
                    connections.insert(token, StreamHandler::with_stream(stream, addr));

                    info!("accepted connection from {addr}");
                },
                token => {
                    let handler = match connections.get_mut(&token) {
                        Some(v) => v,
                        // sporadic events can happen, can safely be ignored
                        None => continue,
                    };
                    if handler.try_rw(event)? {
                        poll.registry().deregister(&mut handler.stream)?;
                        info!("disconnected from {}", handler.addr);
                        connections.remove(&token);
                    }
                }
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}
