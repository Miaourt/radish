use anyhow::Result;
use std::{convert::Infallible, sync::Arc, time::Duration};

use http_body_util::Full;
use hyper::{body::Bytes, server::conn::http1, service::Service, Request, Response};
use smol::{
    future, io,
    net::{SocketAddr, TcpListener, TcpStream},
    stream::StreamExt,
    Timer,
};
use smol_hyper::rt::{FuturesIo, SmolTimer};

enum Events {
    Termination,
    Request(io::Result<(TcpStream, SocketAddr)>),
}

async fn hello(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

#[derive(Debug)]
struct Cache {
    source: String,
}

impl Service<Request<hyper::body::Incoming>> for Cache {
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;
    fn call(&self, _: Request<hyper::body::Incoming>) -> Self::Future {
        future::ready(Ok(Response::new(Full::new(Bytes::from("Hello, World!")))))
    }
}

fn main() -> Result<()> {
    let ex = smol::LocalExecutor::new();

    smol::block_on(ex.run(async {
        let (s, termination_chan) = smol::channel::bounded(1);
        ctrlc::set_handler(move || {
            s.try_send(Events::Termination).unwrap();
        })
        .unwrap();

        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        let listener = TcpListener::bind(addr).await.unwrap();
        println!("Listening to http://{}", addr);

        let cache = Arc::new(Cache {
            source: "".to_string(),
        });

        loop {
            let termination_fut = async {
                termination_chan.recv().await.unwrap();
                Events::Termination
            };
            let req_receiver = async {
                let res = listener.accept().await;
                Events::Request(res)
            };

            let msg = future::or(termination_fut, req_receiver).await;
            match msg {
                Events::Request(Ok((stream, addr))) => {
                    let conn = http1::Builder::new()
                        .timer(SmolTimer::new())
                        .serve_connection(FuturesIo::new(stream), cache.clone());

                    ex.spawn(async move {
                        println!("serving... {}", addr);
                        if let Err(e) = conn.await {
                            eprintln!("Error serving connection: {:?}", e);
                        }
                        Timer::after(Duration::from_secs(5)).await;
                        println!("served ! {}", addr);
                    })
                    .detach();
                }
                Events::Request(Err(e)) => {
                    eprintln!("Error listening to connection: {:?}", e);
                    break;
                }
                Events::Termination => {
                    eprintln!("terminated");
                    break;
                }
            }
        }
    }));

    let timeout_fut = async {
        Timer::after(Duration::from_secs(5)).await;
    };

    let executor_empty_fut = async {
        let mut t = smol::Timer::interval(Duration::from_millis(100));
        loop {
            if ex.is_empty() {
                break;
            }
            t.next().await;
        }
    };

    smol::block_on(ex.run(future::or(timeout_fut, executor_empty_fut)));

    Ok(())
}
