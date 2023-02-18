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
use tracing::{error, info, instrument};

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
            buf: vec![0; 1024],
            to_write: 0,
            addr,
        }
    }

    #[instrument(skip(self, event))]
    fn try_rw(&mut self, event: &Event) -> anyhow::Result<bool> {
        let mut close = false;

        if event.is_readable() {
            close = self.try_read()?;
        }

        self.try_write()?;

        Ok(close)
    }

    #[instrument(skip(self))]
    fn try_read(&mut self) -> anyhow::Result<bool> {
        let StreamHandler {
            stream,
            buf,
            to_write,
            ..
        } = self;
        let old_len = *to_write;
        let mut close = false;

        // read until we are blocked or an error occurs
        loop {
            match stream.read(&mut buf[*to_write..]) {
                Err(e) => match e.kind() {
                    ErrorKind::Interrupted => continue,
                    ErrorKind::WouldBlock => break,
                    _ => return Err(e.into()),
                },

                Ok(n) => {
                    *to_write += n;

                    info!(to_write = to_write, n = n, bl = buf.len());

                    if *to_write == buf.len() {
                        buf.resize(*to_write + 1024, 0);
                        info!("resized bl = {}", buf.len());
                        continue;
                    }

                    if n == 0 {
                        close = true;
                        break;
                    }
                }
            }
        }

        Ok(close || *to_write == old_len)
    }

    #[instrument(skip(self))]
    fn try_write(&mut self) -> anyhow::Result<()> {
        let StreamHandler {
            stream,
            buf,
            to_write,
            addr,
        } = self;

        if *to_write == 0 {
            return Ok(());
        }

        info!("addr = {addr:?} to_write = {to_write}");
        match stream.write_all(&buf[..*to_write]) {
            Ok(()) => *to_write = 0,
            Err(e) if e.kind() != ErrorKind::WouldBlock => return Err(e.into()),
            _ => {}
        }

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = env::args();
    args.next().expect("no binary name provided");
    let socke_addr = args.next().expect("no port number provided").parse()?;

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let poll = &mut Poll::new()?;
    let events = &mut Events::with_capacity(1024);

    let server = &mut TcpListener::bind(socke_addr)?;
    poll.registry().register(
        server,
        SERVER_TOKEN,
        Interest::READABLE | Interest::WRITABLE,
    )?;

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
                        .register(&mut stream, token, Interest::READABLE)?;
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
