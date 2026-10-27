//! The API server ([ADR 0026] §3–4): a blocking accept loop and one thread per connection.
//!
//! Each line is a JSON-RPC request; calls with an `id` get one response line. A client that
//! calls `events.subscribe` also receives every [`Events::publish`] as a notification line on
//! the same connection, until it disconnects.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};

use crate::proto::{
    INVALID_REQUEST, MAX_LINE, Notification, PARSE_ERROR, Request, Response, RpcError,
};
use crate::transport::{Listener, Stream};

/// Answers calls. Implementations are shared by every connection's thread.
pub trait Handler: Send + Sync {
    fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError>;
}

/// The clients subscribed to events.
#[derive(Debug, Default)]
pub struct Events {
    subscribers: Mutex<Vec<Sender<String>>>,
}

impl Events {
    /// Sends `method(params)` to every subscriber; ones that have gone are dropped.
    pub fn publish(&self, method: &str, params: Value) {
        let Ok(line) = serde_json::to_string(&Notification::new(method, params)) else {
            return;
        };
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|subscriber| subscriber.send(line.clone()).is_ok());
    }

    fn subscribe(&self) -> mpsc::Receiver<String> {
        let (sender, receiver) = mpsc::channel();
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(sender);
        receiver
    }
}

/// Accepts connections on `listener` until it fails, serving each on its own thread.
pub fn serve(listener: &Listener, handler: &Arc<dyn Handler>, events: &Arc<Events>) -> io::Error {
    loop {
        match listener.accept() {
            Ok(stream) => {
                let handler = Arc::clone(handler);
                let events = Arc::clone(events);
                std::thread::spawn(move || {
                    let _ = serve_connection(stream, handler.as_ref(), &events);
                });
            }
            Err(e) => return e,
        }
    }
}

/// Serves one connection until the client closes it.
pub fn serve_connection(stream: Stream, handler: &dyn Handler, events: &Events) -> io::Result<()> {
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        // One byte over the limit tells a too-long line from one that just fits.
        let read = (&mut reader)
            .take(MAX_LINE as u64 + 1)
            .read_line(&mut line)?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > MAX_LINE {
            let response = Response::new(
                Value::Null,
                Err(RpcError::new(INVALID_REQUEST, "line too long")),
            );
            write_line(&writer, &response)?;
            return Ok(());
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(text) {
            Ok(request) => request,
            Err(e) => {
                let response =
                    Response::new(Value::Null, Err(RpcError::new(PARSE_ERROR, e.to_string())));
                write_line(&writer, &response)?;
                continue;
            }
        };
        let outcome = if request.jsonrpc != "2.0" {
            Err(RpcError::new(INVALID_REQUEST, "jsonrpc must be \"2.0\""))
        } else if request.method == "events.subscribe" {
            let receiver = events.subscribe();
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || {
                for note in receiver {
                    let mut stream = writer.lock().unwrap_or_else(PoisonError::into_inner);
                    if writeln!(stream, "{note}")
                        .and_then(|()| stream.flush())
                        .is_err()
                    {
                        return;
                    }
                }
            });
            Ok(json!({"subscribed": true}))
        } else {
            handler.call(&request.method, &request.params)
        };
        if let Some(id) = request.id {
            write_line(&writer, &Response::new(id, outcome))?;
        }
    }
}

fn write_line(writer: &Mutex<Stream>, response: &Response) -> io::Result<()> {
    let text = serde_json::to_string(response).map_err(io::Error::other)?;
    let mut stream = writer.lock().unwrap_or_else(PoisonError::into_inner);
    writeln!(stream, "{text}")?;
    stream.flush()
}
