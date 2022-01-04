use futures::prelude::*;
use tokio_serde::formats::*;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Message {
    Ok,
    Terminate
}

#[tokio::main]
pub async fn main() -> Result<(), failure::Error> {
    let (a, b) = tokio::io::duplex(100);

    let frame_a = Framed::new(a, LengthDelimitedCodec::new());
    let mut ser_a = tokio_serde::SymmetricallyFramed::new(
        frame_a, SymmetricalCbor::default()
    );

    let frame_b = Framed::new(b, LengthDelimitedCodec::new());
    let mut ser_b = tokio_serde::SymmetricallyFramed::new(
        frame_b, SymmetricalCbor::default()
    );

    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Ok).await?;
    ser_a.send(Message::Terminate).await?;

    // Get a single message
    let v: Message = ser_b.try_next().await?.unwrap();
    println!("first: {:?}", v);

    // loop through the remaining messages using select
    loop {
        tokio::select! {
            x = ser_a.try_next() => {
                match x? {
                    Some(v) => {
                        println!("Echo: {:?}", v);
                    }
                    None => break,
                }
            }

            x = ser_b.try_next() => {
                match x? {
                    Some(Message::Terminate) => {
                        println!("terminate");
                        break;
                    }
                    Some(v) => {
                        println!("received {:?}", v);
                        ser_b.send(v).await?;
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}
