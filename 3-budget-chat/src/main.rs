use std::{collections::HashSet, env, net::SocketAddr, sync::Arc};

use anyhow::anyhow;
use futures::{stream::FuturesUnordered, StreamExt};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{broadcast, RwLock},
};
use tracing::{error, info};

pub type Message = Arc<String>;
pub type UserName = Arc<String>;
pub type UsersList = Arc<RwLock<HashSet<Arc<String>>>>;

const WELCOME_MESSAGE: &[u8] = b"* Welcome to budgetchat! What shall I call you?\n";
const LONG_NAME_ERR_MESSAGE: &[u8] = b"* name is to long, atmost 16 characters allowed\n";
const DUPLICATE_NAME_ERR_MESSAGE: &[u8] = b"* this name is already in use\n";

macro_rules! log_and_exit {
    ($addr:ident) => {
        info!("closing connection with {}", $addr);
        return Ok(());
    };
}

macro_rules! write_and_exit {
    ($writer:ident, $msg:ident, $addr:ident) => {
        $writer.write_all($msg).await?;
        log_and_exit!($addr);
    };
}

fn create_arrival_message(name: &str) -> Message {
    Message::new(format!("* {name} has entered the room\n"))
}

fn create_departure_message(name: &str) -> Message {
    Message::new(format!("* {name} has left the room\n"))
}

fn create_current_users_message(users: &[UserName]) -> String {
    if users.len() == 0 {
        return "* no users in room\n".to_string();
    }

    let mut res = format!("* the room contains: {}", users[0]);
    for user in users.into_iter().skip(1) {
        res.push_str(", ");
        res.push_str(&user);
    }

    res.push('\n');

    return res;
}

async fn handle_stream(
    mut stream: TcpStream,
    addr: SocketAddr,
    tx: broadcast::Sender<Message>,
    users: UsersList,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.split();
    let mut lines = BufReader::new(reader).lines();

    // greet the user

    writer.write_all(WELCOME_MESSAGE).await?;

    // get the username

    let name = lines
        .next_line()
        .await?
        .ok_or_else(|| anyhow!("no userame given\n"))?;

    // validate the username

    if name.is_empty() || name.chars().any(|c| !c.is_alphanumeric()) || name.len() > 16 {
        write_and_exit!(writer, LONG_NAME_ERR_MESSAGE, addr);
    }

    let name = Arc::new(name);

    let current_users = {
        let lock = users.read().await;

        if lock.contains(&name) {
            write_and_exit!(writer, DUPLICATE_NAME_ERR_MESSAGE, addr);
        }

        lock.iter().map(Arc::clone).collect::<Vec<_>>()
    };

    // make presence noticed, append to UsersList

    {
        let lock = &mut users.write().await;
        let arrival_message = create_arrival_message(&name);
        tx.send(arrival_message.clone())?;
        lock.insert(name.clone());
    }

    // send back list of all current users

    writer
        .write_all(create_current_users_message(&current_users).as_bytes())
        .await?;

    // chat messages

    let mut rx = tx.subscribe();
    loop {
        tokio::select! {
            res_msg_opt = lines.next_line() => {
                let msg = match res_msg_opt? {
                    Some(msg) =>  Arc::new(format!("[{name}]: {}\n", msg.trim())),
                    None => break,
                };
                tx.send(msg)?;
            }
            res_msg = rx.recv() => {
                let msg = res_msg?;
                writer.write_all(msg.as_bytes()).await?;
            }
        }
    }

    // make absence notices, remove from UsersList

    {
        let lock = &mut users.write().await;
        let departure_message = create_departure_message(&name);
        tx.send(departure_message.clone())?;
        lock.remove(&name);
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

    let mut connections = FuturesUnordered::new();
    let (tx, _rx) = broadcast::channel(1024);
    let users = UsersList::new(RwLock::new(HashSet::new()));

    loop {
        tokio::select! {
            res = server.accept() => {
                let (stream, addr) = res?;
                info!("accepted connection from {addr}");
                connections.push(handle_stream(
                    stream,
                    addr,
                    tx.clone(),
                    users.clone()
                ));
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
