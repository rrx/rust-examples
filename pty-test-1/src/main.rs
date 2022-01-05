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
use std::process::Stdio;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Message {
    Ok,
    Terminate
}

async fn test1() -> Result<(), failure::Error> {
    use pty_test::test::*;
    let (mut master, slave) = openpty(PtySize::default())?;

    // these need to stick around for some reason
    //let mut reader1 = PtyFd::from_fd(master.fd.try_clone()?)?;
    //let mut reader2 = PtyFd::from_fd(slave.fd.try_clone()?)?;

    let mut argv = std::env::args().collect::<Vec<_>>();
    let mut part1 = argv.split_off(1);
    if part1.len() == 0 {
        return Ok(());//Err("No params");
    }
    let args = part1.split_off(1);
    let cmd = part1.get(0).unwrap();
    log::info!("{:?}", (&cmd,&args));

    let mut command = tokio::process::Command::new(cmd);
    command.kill_on_drop(true);
    command.args(args);

    let (mut stdout_a, stdout_b) = socketpair::tokio_socketpair_stream().await?;
    let (stderr_a, stderr_b) = socketpair::tokio_socketpair_stream().await?;
    let (stdin_a, stdin_b) = socketpair::tokio_socketpair_stream().await?;
    //let (stdin_a, stdin_b) = tokio::io::duplex(100);
    //let (stdin_a, stdin_b) = std::os::unix::net::UnixStream::pair()?;
    //let (stdin_a, stdin_b) = os_pipe::pipe()?;
    //stdin_a.set_nonblocking(false);
    //stdin_b.set_nonblocking(false);
    //let c = stdout_a.into_std().try_clone()?;

    //let stdin = slave.fd.as_stdio()?;
    //println!("x{:?}", stdin);
    let stdin = unsafe { std::process::Stdio::from_raw_fd(stdin_a.as_raw_fd()) };
    let stdin_a = unsafe { std::process::Stdio::from_raw_fd(stdin_a.as_raw_fd()) };
    command.stdin(stdin);
    let stdout = unsafe { std::process::Stdio::from_raw_fd(stdout_a.as_raw_fd()) };
    command.stdout(stdout);
    let stderr = unsafe { std::process::Stdio::from_raw_fd(stderr_a.as_raw_fd()) };
    command.stderr(stderr);

    //let master_stdout = unsafe { std::process::Stdio::from_raw_fd(master.fd.as_raw_fd()) };

    //let dup_stdin = os_pipe::dup_stdin()?;
    //command.stdin(dup_stdin);
    //let other = slave.fd.try_clone()?;

    command.stdin(slave.fd.as_stdio()?);
    //command.stdin(master.fd.as_stdio()?);
    //command.stdin(Stdio::null());
    //command.stdin(Stdio::piped());
    //command.stdin(Stdio::inherit());
    //let mut reader = master.try_clone_reader()?;
    //let reader = slave.fd.try_clone()?.as_stdio()?;
    //command.stdin(reader);
    //let status = child.wait().await?;
    //let status = child.wait().await?;
    command.stdout(slave.fd.as_stdio()?);
    command.stderr(slave.fd.as_stdio()?);

    let mut child = slave.spawn_command(command)?;
    //drop(slave);
    //drop(master);

    println!("{:?}", child);
    //println!("{:?}", reader2);
    //let stdin = child.stdin.take().unwrap();
    //let stdout = child.stdout.take().unwrap();
    //let stderr = child.stderr.take().unwrap();

    //let r = tokio::io::AsyncReadExt::read(&mut master.fd, &mut buffer[..]).await;

    // This writes synchronously
    let xy = std::io::Write::write(&mut master.fd, b"asdf\n");
    println!("xy {:?}", (xy));
    std::io::Write::flush(&mut master.fd);

    let xx = std::io::Write::write(&mut master.fd, b"asdf\n");
    println!("xx {:?}", (xx));
    std::io::Write::flush(&mut master.fd);

    // we are able to read like this, but it's sync
    let mut buffer = [0;100];
    if let Ok(r) = std::io::Read::read(&mut master.fd, &mut buffer[..]) {
        println!("r{:?}", (r, &buffer[..r]));
    }
                 
    
    //let mut framed_stdin = codec::FramedWrite::new(other.as_stdio()?, codec::BytesCodec::new());
    let mut framed_stdin = codec::FramedWrite::new(stdin_b, codec::BytesCodec::new());
    //let mut framed_stdout = codec::FramedRead::new(slave.fd, codec::BytesCodec::new());
    let mut framed_stdout = codec::FramedRead::new(stdout_b, codec::BytesCodec::new());
    //let mut framed_stdout = codec::FramedRead::new(master_stdout, codec::BytesCodec::new());
    let mut framed_stderr = codec::FramedRead::new(stderr_b, codec::BytesCodec::new());

    framed_stdin.send(bytes::Bytes::from("asdf\n\n")).await;

    loop {
        tokio::select! {
            x = framed_stdout.try_next() => {
                match x {
                    Ok(None) => break,
                    Ok(Some(v)) => print!("{:?}", v),
                    Err(e) => print!("{:?}", e),
                    _ => ()

                }
            }

            x = framed_stderr.try_next() => {
                match x {
                    Ok(None) => break,
                    Ok(Some(v)) => print!("ERR: {:?}", v),
                    Err(e) => print!("ERR: {:?}", e),
                    _ => ()

                }
            }
            // break out if wait returns
            r = child.wait() => {
                if let Ok(status) = r {
                    //stdout_a.flush();
                    log::info!("child status: {:?}", (status.success(), status.code(), status));
                    //break
                }
            }
        }
    }

    // make sure we get anything that remains
    if let Ok(Some(x)) = framed_stdout.try_next().await {
        println!("{:?}", x);
    }
    if let Ok(Some(x)) = framed_stderr.try_next().await {
        println!("{:?}", x);
    }

    // the last thing we do is wait for the child to exit
    child.wait().await;

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
    env_logger::init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(test1())
    //rt.block_on(test2())

}
