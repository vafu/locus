use anyhow::{Context, bail};
use niri_ipc::{Request, Response, socket::Socket};
use tokio::sync::mpsc;

pub fn event_stream() -> anyhow::Result<mpsc::Receiver<niri_ipc::Event>> {
    let mut socket = Socket::connect().context("connect to Niri IPC socket")?;
    match socket
        .send(Request::EventStream)
        .context("request Niri event stream")?
    {
        Ok(Response::Handled) => {}
        Ok(response) => bail!("unexpected Niri response to event stream request: {response:?}"),
        Err(message) => bail!("Niri rejected event stream request: {message}"),
    }

    let (tx, rx) = mpsc::channel(128);
    std::thread::Builder::new()
        .name("niri-event-stream".to_string())
        .spawn(move || {
            let mut read_event = socket.read_events();
            loop {
                match read_event() {
                    Ok(event) => {
                        if tx.blocking_send(event).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        eprintln!("locus-niri: failed to read Niri event stream: {error}");
                        break;
                    }
                }
            }
        })
        .context("spawn Niri event stream reader")?;

    Ok(rx)
}
