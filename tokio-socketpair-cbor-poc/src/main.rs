use futures::prelude::*;
use std::io::{Read, Write};
use std::io;
use std::os::unix::io::{RawFd, AsRawFd};
use tokio::io::AsyncReadExt;
use tokio_serde::formats::*;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Message {
    Ok,
    Terminate
}

fn main() -> Result<(), failure::Error> {
    test_mio()?;
    test_tokio()?;
    Ok(())
}

async fn test_tokio_async() -> Result<(), failure::Error> {
    // Async socket pair
    let (mut a, mut b) = socketpair::tokio_socketpair_stream().await.unwrap();
    let frame_a = Framed::new(a, LengthDelimitedCodec::new());
    let mut ser_a = tokio_serde::SymmetricallyFramed::new(
        frame_a, SymmetricalCbor::default()
    );
    let frame_b = Framed::new(b, LengthDelimitedCodec::new());
    let mut ser_b = tokio_serde::SymmetricallyFramed::new(
        frame_b, SymmetricalCbor::default()
    );
    ser_b.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Terminate).await?;

    loop {
        tokio::select! {
            x = ser_a.try_next() => {
                println!("read a{:?}", (x));
            }
            x = ser_b.try_next() => {
                println!("read b{:?}", (x));
            }
        }
    }
    Ok(())
}

fn test_tokio() -> Result<(), failure::Error> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(test_tokio_async())
}

fn test_mio() -> Result<(), failure::Error> {
    // Sync socket pair
    let (mut a, mut b) = socketpair::socketpair_stream()?;

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);

    poll.registry().register(
        &mut mio::unix::SourceFd(&a.as_raw_fd()),
        Token(0), Interest::READABLE | Interest::WRITABLE)?;

    poll.registry().register(
        &mut mio::unix::SourceFd(&b.as_raw_fd()),
        Token(1), Interest::READABLE | Interest::WRITABLE)?;

    poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))?;

    writeln!(b, "hello")?;
    writeln!(a, "hello")?;
    let mut buf = String::new();

    for x in events.iter() {
        println!("{:?}", (x.token()));
        match x.token() {
            Token(0) if x.is_readable() => {
                a.read_to_string(&mut buf)?;
                println!("{:?}", (buf));
            }
            Token(1) if x.is_readable() => {
                let mut buf = String::new();
                b.read_to_string(&mut buf)?;
                println!("{:?}", (buf));
            }
            _ => {
                println!("{:?}", (x.token(), x.is_readable(), x.is_writable()));
            }
        }
    }

    Ok(())
}
