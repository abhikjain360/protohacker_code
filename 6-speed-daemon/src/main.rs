use std::{collections::BTreeMap, io::IoSlice, sync::Arc, time::Duration};

use anyhow::anyhow;
use futures::future;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{mpsc, Mutex, RwLock},
    time,
};
use tracing::error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    util::accept_loop_with_env(handle_stream, State::default()).await
}

#[derive(Clone, Default)]
struct State {
    cars: Arc<Mutex<CarsMap>>,
    dispatchers: Arc<RwLock<DispatchersMap>>,
}

type Map<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;
type Road = u16;
type Plate = Arc<Vec<u8>>;
type Timestamp = u32;
type Miles = u16;

type CarsMap = Map<Plate, Car>;

#[derive(Default)]
struct Car {
    roads: RoadsMap,
    tickets: TicketsSet,
}

type RoadsMap = Map<Road, Entries>;
type Entries = BTreeMap<Timestamp, Miles>;

type TicketsSet = indexmap::IndexSet<Day>;
type Day = Timestamp;

#[derive(Debug)]
struct Ticket {
    plate: Arc<Vec<u8>>,
    road: u16,
    mile1: u16,
    timestamp1: u32,
    mile2: u16,
    timestamp2: u32,
    speed: u16,
}

impl Ticket {
    fn new(
        plate: Arc<Vec<u8>>,
        road: u16,
        mile1: u16,
        timestamp1: u32,
        mile2: u16,
        timestamp2: u32,
        speed: f64,
    ) -> Self {
        Self {
            plate,
            road,
            mile1,
            timestamp1,
            mile2,
            timestamp2,
            speed: (speed * 100.0) as u16,
        }
    }
}

macro_rules! ticket_io_slices {
    ($ticket:ident) => {
        &[
            IoSlice::new(&[$ticket.plate.len() as u8]),
            IoSlice::new(&$ticket.plate),
            IoSlice::new(&$ticket.road.to_be_bytes()),
            IoSlice::new(&$ticket.mile1.to_be_bytes()),
            IoSlice::new(&$ticket.timestamp1.to_be_bytes()),
            IoSlice::new(&$ticket.mile2.to_be_bytes()),
            IoSlice::new(&$ticket.timestamp2.to_be_bytes()),
            IoSlice::new(&$ticket.speed.to_be_bytes()),
        ]
    };
}

#[derive(Default)]
struct DispatchersMap {
    roads_map: Map<Road, Dispatchers>,
    dispatchers: Map<DispatchersId, mpsc::Sender<Ticket>>,
    pending_tickets: Map<Road, Vec<Ticket>>,
    last_id: DispatchersId,
}

type Dispatchers = indexmap::IndexSet<DispatchersId>;

struct DispatcherInsert {
    pending_tickets: Vec<Ticket>,
    rx: mpsc::Receiver<Ticket>,
}

impl DispatchersMap {
    fn insert(&mut self, roads: Vec<u16>) -> anyhow::Result<DispatcherInsert> {
        let dispatcher_id = self.last_id;
        self.last_id += 1;

        let (tx, rx) = mpsc::channel(1024);

        for road in &roads {
            self.roads_map
                .entry(*road)
                .or_default()
                .insert(dispatcher_id);
        }

        self.dispatchers.insert(dispatcher_id, tx);

        let pending_tickets = roads
            .into_iter()
            .filter_map(|road| self.pending_tickets.remove(&road))
            .flat_map(|v| v.into_iter())
            .collect();

        Ok(DispatcherInsert {
            rx,
            pending_tickets,
        })
    }

    async fn send_ticket(
        &mut self,
        plate: Arc<Vec<u8>>,
        road: u16,
        mile1: u16,
        timestamp1: u32,
        mile2: u16,
        timestamp2: u32,
        speed: f64,
    ) -> Result<(), mpsc::error::SendError<Ticket>> {
        let ticket = Ticket::new(plate, road, mile1, timestamp1, mile2, timestamp2, speed);

        if let Some(dispatcher_id) = self
            .roads_map
            .get(&road)
            .and_then(|dispatchers| dispatchers.first())
        {
            if let Some(tx) = self.dispatchers.get(dispatcher_id) {
                return tx.send(ticket).await;
            }
            error!("{dispatcher_id} dispatcher in roads_map but does not exist");
        }

        self.pending_tickets.entry(road).or_default().push(ticket);
        Ok(())
    }
}

type DispatchersId = u16;

struct HeartbeatTimer {
    next: time::Instant,
    interval: Duration,
}
struct Heartbeat(Option<HeartbeatTimer>);

impl Heartbeat {
    fn from_interval(interval: u64) -> Self {
        if interval != 0 {
            let interval = Duration::from_millis(interval as u64 * 100);
            Self(Some(HeartbeatTimer {
                next: time::Instant::now() + interval,
                interval,
            }))
        } else {
            Self(None)
        }
    }

    async fn wait(&mut self) {
        match &mut self.0 {
            Some(timer) => {
                time::sleep_until(timer.next).await;
                timer.next = time::Instant::now() + timer.interval;
            },
            None => future::pending().await,
        }
    }
}

const ERROR: u8 = 0x10;

const PLATE: u8 = 0x20;

const WANT_HEARTBEAT: u8 = 0x40;
const HEARTBEAT: u8 = 0x41;

const I_AM_CAMERA: u8 = 0x80;
const I_AM_DISPATCHER: u8 = 0x81;

async fn handle_stream(mut stream: TcpStream, state: State) -> anyhow::Result<()> {
    let mut heartbeat = Heartbeat(None);

    loop {
        tokio::select! {
            msg_type_res = stream.read_u8() => {
                match msg_type_res? {
                    WANT_HEARTBEAT => {
                        let interval = stream.read_u32().await?;
                        heartbeat = Heartbeat::from_interval(interval as u64);
                        continue;
                    }

                    I_AM_CAMERA => return camera(stream, state, heartbeat).await,
                    I_AM_DISPATCHER => return dispatcher(stream, state, heartbeat).await,

                    msg_type => return Err(anyhow!("invalid msg type: {msg_type}")),
                }
            },
            _ = heartbeat.wait() => {
                stream.write_u8(HEARTBEAT).await?;
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}

async fn camera(
    mut stream: TcpStream,
    state: State,
    mut heartbeat: Heartbeat,
) -> anyhow::Result<()> {
    let road = stream.read_u16().await?;
    let mile = stream.read_u16().await?;
    let limit = stream.read_u16().await? as f64;

    loop {
        tokio::select! {
            msg_type_res = stream.read_u8() => {
                match msg_type_res? {
                    WANT_HEARTBEAT => {
                        let interval = stream.read_u32().await?;
                        heartbeat = Heartbeat::from_interval(interval as u64);
                        continue;
                    }

                    PLATE => {
                        let plate = read_str(&mut stream).await?;
                        let timestamp = stream.read_u32().await?;

                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_plate(plate, timestamp, road, mile, limit, state).await {
                                error!("{e}");
                            }
                        });
                    }

                    msg_type => return send_error(&mut stream, msg_type).await,
                }
            },
            _ = heartbeat.wait() => {
                stream.write_u8(HEARTBEAT).await?;
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}

async fn handle_plate(
    plate: Plate,
    timestamp: u32,
    road: u16,
    mile: u16,
    limit: f64,
    state: State,
) -> anyhow::Result<()> {
    let mut lock = state.cars.lock().await;

    let car = lock.entry(plate.clone()).or_default();
    let timestamps = car.roads.entry(road).or_default();
    let day = timestamp / 86400;
    let mut ticket_recved = !car.tickets.contains(&day);

    if let Some((previous_timestamp, previous_mile)) = timestamps.range(..timestamp).next_back() {
        if !ticket_recved
            && check_speed(
                plate.clone(),
                *previous_timestamp,
                *previous_mile,
                timestamp,
                mile,
                road,
                limit,
                &state,
            )
            .await?
        {
            car.tickets.insert(day);
            ticket_recved = true;
        }
    }

    if let Some((previous_timestamp, previous_mile)) = timestamps.range(timestamp..).next() {
        if !ticket_recved
            && check_speed(
                plate,
                *previous_timestamp,
                *previous_mile,
                timestamp,
                mile,
                road,
                limit,
                &state,
            )
            .await?
        {
            car.tickets.insert(day);
        }
    }

    timestamps.insert(timestamp, mile);

    Ok(())
}

async fn check_speed(
    plate: Plate,
    timestamp1: u32,
    mile1: u16,
    timestamp2: u32,
    mile2: u16,
    road: u16,
    limit: f64,
    state: &State,
) -> anyhow::Result<bool> {
    let time = timestamp1 as f64 - timestamp2 as f64;
    let distance = mile1 as f64 - mile2 as f64;
    let speed = (time / distance).abs();
    if speed - limit >= 0.5 {
        state
            .dispatchers
            .write()
            .await
            .send_ticket(plate, road, mile1, timestamp1, mile2, timestamp2, speed)
            .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn dispatcher(
    mut stream: TcpStream,
    state: State,
    mut heartbeat: Heartbeat,
) -> anyhow::Result<()> {
    let numroads = stream.read_u8().await?;
    let mut roads = Vec::with_capacity(numroads as usize);
    for _ in 0..numroads {
        roads.push(stream.read_u16().await?);
    }

    let DispatcherInsert {
        pending_tickets,
        mut rx,
    } = state.dispatchers.write().await.insert(roads)?;

    for ticket in pending_tickets {
        stream.write_vectored(ticket_io_slices!(ticket)).await?;
    }

    loop {
        tokio::select! {
            msg_type_res = stream.read_u8() => {
                match msg_type_res? {
                    WANT_HEARTBEAT => {
                        let interval = stream.read_u32().await?;
                        heartbeat = Heartbeat::from_interval(interval as u64);
                        continue;
                    }

                    msg_type => return Err(anyhow!("invalid msg type: {msg_type}")),
                }
            },

            msg_opt = rx.recv() => {
                let Some(ticket) = msg_opt else {
                    break;
                };
                stream.write_vectored(ticket_io_slices!(ticket)).await?;
            }

            _ = heartbeat.wait() => {
                stream.write_u8(HEARTBEAT).await?;
            }
        }
    }

    #[allow(unreachable_code)]
    Ok(())
}

async fn send_error(stream: &mut TcpStream, msg_type: u8) -> anyhow::Result<()> {
    stream
        .write_vectored(&[IoSlice::new(&[ERROR]), IoSlice::new(&[0])])
        .await?;
    Err(anyhow!("invalid msg type: {msg_type}"))
}

async fn read_str(stream: &mut TcpStream) -> anyhow::Result<Arc<Vec<u8>>> {
    let len = stream.read_u8().await?;
    let mut buf = vec![0; len as usize];
    stream.read_exact(&mut buf).await?;
    Ok(Arc::new(buf))
}
