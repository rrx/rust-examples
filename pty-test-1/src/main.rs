use futures::prelude::*;
use std::io::{Read, Write};
use tokio_util::codec;
use tokio_util::compat::*;
use serde::{Serialize, Deserialize};
use tokio_serde::formats::*;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use std::os::unix::io::FromRawFd;
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::RawFd;
use std::io;
use std::os::unix::process::CommandExt;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Message {
    Ok,
    Terminate
}

async fn test1() -> Result<(), failure::Error> {
    use pty_test::test::*;
    let (master, slave) = openpty(PtySize::default())?;
    //let mut reader = master.try_clone_reader()?;
    let mut reader1 = PtyFd::from_fd(master.fd.try_clone()?)?;
    let mut reader2 = PtyFd::from_fd(slave.fd.try_clone()?)?;
    //
    //let fd = unsafe {
        //std::fs::File::from_raw_fd(reader.as_raw_fd())
    //};

    //let file = tokio_file_unix::File::new_nb(fd.as_raw_fd())?;//master.fd.as_raw_fd());
    let mut command = tokio::process::Command::new("tty");

    let (stdout_a, stdout_b) = socketpair::tokio_socketpair_stream().await?;
    let (stderr_a, stderr_b) = socketpair::tokio_socketpair_stream().await?;
    let (stdin_a, stdin_b) = socketpair::tokio_socketpair_stream().await?;

    //let stdin = slave.fd.as_stdio()?;
    //println!("x{:?}", stdin);
    let stdin = unsafe { std::process::Stdio::from_raw_fd(stdin_a.as_raw_fd()) };
    //command.stdin(stdin);
    let stdout = unsafe { std::process::Stdio::from_raw_fd(stdout_a.as_raw_fd()) };
    command.stdout(stdout);
    let stderr = unsafe { std::process::Stdio::from_raw_fd(stderr_a.as_raw_fd()) };
    //command.stderr(stderr);
 
    command.stdin(slave.fd.try_clone()?.as_stdio()?);
    //command.stdout(slave.fd.try_clone()?.as_stdio()?);
    command.stderr(slave.fd.try_clone()?.as_stdio()?);

    let mut child = slave.spawn_command(command)?;
    drop(slave);
    drop(master);

    println!("{:?}", child);
    println!("{:?}", reader2);
    //let stdin = child.stdin.take().unwrap();
    //let stdout = child.stdout.take().unwrap();
    //let stderr = child.stderr.take().unwrap();

    //let mut framed = codec::FramedRead::new(stdout, codec::LinesCodec::new());
    let mut framed_stdin = codec::FramedRead::new(stdin_b, codec::BytesCodec::new());
    let mut framed_stdout = codec::FramedRead::new(stdout_b, codec::BytesCodec::new());
    let mut framed_stderr = codec::FramedRead::new(stderr_b, codec::BytesCodec::new());

    //let x = framed_stdout.try_next().await?;
    //println!("{:?}", x);

    //let mut r = FramedRead::new(Pin::new(reader), BytesCodec::new());
    //let mut s = String::new();
    //reader.read_to_string(&mut s)?;
    //print!("output: ");
    //for c in s.escape_debug() {
        //print!("{}", c);
    //}
    //print!("\n");
    let status = child.wait().await?;
    println!("child status: {:?}", status);
    Ok(())
}


async fn test2() -> Result<(), failure::Error> {
    use futures::prelude::*;
    use tokio_serde::formats::*;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};
    use serde::{Serialize, Deserialize};
    use tokio_util::compat::*;

    // convert stdin into a nonblocking file;
    // this is the only part that makes use of tokio_file_unix
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let stderr = tokio::io::stderr();

    //let (stdin, stdout) = tokio::io::duplex(100);

    //let file = tokio_file_unix::raw_stdin()?;
    //let file = tokio_file_unix::File::new_nb(file)?;

    //let frame_stdin = FramedRead::new(stdin, LengthDelimitedCodec::new());
    //let mut ser_a = tokio_serde::SymmetricallyFramed::new(
        //frame_stdin, SymmetricalCbor::default());
        //
    //let mut r = FramedRead::new(Pin::new(reader), BytesCodec::new());

    let mut framed_in = codec::FramedRead::new(stdin, codec::BytesCodec::new());
    let mut framed_out = codec::FramedWrite::new(stdout, codec::BytesCodec::new());
    let mut framed_err = codec::FramedWrite::new(stderr, codec::BytesCodec::new());

    while let Some(got) = framed_in.try_next().await? {
        println!("Got this: {:?}", got);
        let b = got.freeze();
        framed_out.send(b.clone()).await;
        framed_err.send(b).await;
    }

    println!("Received None, lol");
    Ok(())
}

fn main() -> Result<(), failure::Error> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(test1())
    //rt.block_on(test2())

}
