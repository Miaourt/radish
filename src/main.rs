use anyhow::Result;
use std::time::Duration;

use smol::{
    future,
    io::{self, AsyncReadExt},
    net::{SocketAddr, TcpListener, TcpStream},
    stream::StreamExt,
    Timer,
};

enum Events {
    Termination,
    Request(io::Result<(TcpStream, SocketAddr)>),
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

            let msg = future::or(termination_fut, req_receiver).await;
            match msg {
                Events::Request(Ok((mut stream, addr))) => {
                    ex.spawn(async move {
                        println!("serving... {}", addr);

                        let mut buf = [0; 4096];
                        let mut headers = [httparse::EMPTY_HEADER; 64];
                        let mut req = httparse::Request::new(&mut headers);

                        let bytes_read = stream.read(&mut buf).await.unwrap();

                        match req.parse(&buf[..bytes_read]) {
                            Ok(_) => {
                                println!("{:?}", req);
                            }
                            Err(e) => println!("Failed to parse: {}", e),
                        }

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
