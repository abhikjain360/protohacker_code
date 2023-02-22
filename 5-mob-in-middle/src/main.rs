use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const UPSTREAM_ADDR: &str = "chat.protohackers.com:16963";
const TONY_WALLET: &[u8] = b"7YWHMfk9JZe0LM0g1ZauHuiSxhI";

fn is_wallet_addr(segment: &[u8]) -> bool {
    segment.len() >= 26
        && segment.len() <= 35
        && segment[0] == b'7'
        && segment.iter().all(u8::is_ascii_alphanumeric)
}

fn replace_wallet(message: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(message.len());
    let mut i = 0;

    while i < message.len() {
        let Some(pos) = message[i..].iter().position(|b| !b.is_ascii_whitespace()) else {
            res.extend_from_slice(&message[i..]);
            break;
        };
        let start = i + pos;
        res.extend_from_slice(&message[i..start]);

        let end = match message[start..].iter().position(u8::is_ascii_whitespace) {
            Some(pos) => start + pos,
            None => message.len(),
        };

        let segment = &message[start..end];

        if is_wallet_addr(segment) {
            res.extend_from_slice(TONY_WALLET);
        } else {
            res.extend_from_slice(segment);
        }

        i = end;
    }

    #[allow(unreachable_code)]
    res
}

async fn handle_stream(mut stream: TcpStream, _: ()) -> anyhow::Result<()> {
    let mut upstream = TcpStream::connect(UPSTREAM_ADDR).await?;
    let (upstream_reader, mut upstream_writer) = upstream.split();
    let upstream_buf = &mut Vec::new();
    let mut upstream_lines = BufReader::new(upstream_reader);

    let (client_reader, mut client_writer) = stream.split();
    let client_buf = &mut Vec::new();
    let client_msg_buf = &mut Vec::new();
    let mut client_lines = BufReader::new(client_reader);

    loop {
        tokio::select! {
            res = client_lines.read_until(b'\n', client_buf) => {
                let n = res?;
                if n == 0 {
                    break;
                }
                client_msg_buf.extend_from_slice(&client_buf);
                client_buf.clear();
                if *client_msg_buf.last().unwrap() != b'\n' {
                    continue;
                }
                upstream_writer.write_all(&replace_wallet(&client_msg_buf)).await?;
                client_msg_buf.clear();
            }
            res = upstream_lines.read_until(b'\n', upstream_buf) => {
                let n = res?;
                if n == 0 {
                    break;
                }
                client_writer.write_all(&replace_wallet(&upstream_buf)).await?;
                upstream_buf.clear();
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    util::accept_loop_with_env(handle_stream, ()).await
}
