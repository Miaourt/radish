use anyhow::Result;
use std::{convert::Infallible, time::Duration};

use http_body_util::Full;
use hyper::{body::Bytes, server::conn::http1, service::service_fn, Request, Response};
use smol::{
    future::FutureExt,
    io,
    net::{SocketAddr, TcpListener, TcpStream},
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

        loop {
            let termination_fut = async {
                termination_chan.recv().await.unwrap();
                Events::Termination
            };
            let req_receiver = async {
                let res = listener.accept().await;
                Events::Request(res)
            };

            let msg = termination_fut.or(req_receiver).await;
            match msg {
                Events::Request(req) => match req {
                    Ok((stream, addr)) => {
                        let conn = http1::Builder::new()
                            .timer(SmolTimer::new())
                            .serve_connection(FuturesIo::new(stream), service_fn(hello));

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
                    Err(e) => {
                        eprintln!("Error listening to connection: {:?}", e);
                        break;
                    }
                },
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
        loop {
            if ex.is_empty() {
                break;
            }
            Timer::after(Duration::from_millis(100)).await;
        }
    };

    smol::block_on(ex.run(timeout_fut.or(executor_empty_fut)));

    Ok(())
}
